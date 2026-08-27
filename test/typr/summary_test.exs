defmodule Typr.SummaryTest do
  use ExUnit.Case, async: true

  alias Typr.Summary

  defp result(overrides) do
    Map.merge(
      %{
        at: days_ago(0),
        mode: :time,
        limit: 30,
        list: "english",
        punctuation: false,
        numbers: false,
        config: "time-30-english",
        wpm: 70.0,
        raw: 75.0,
        accuracy: 96.0,
        consistency: 80.0,
        correct: 175,
        incorrect: 5,
        extra: 0,
        missed: 0,
        duration_ms: 30_000,
        keys: %{},
        slips: %{}
      },
      Map.new(overrides)
    )
  end

  defp days_ago(days) do
    :calendar.local_time()
    |> NaiveDateTime.from_erl!()
    |> NaiveDateTime.add(-days * 86_400, :second)
    |> NaiveDateTime.to_iso8601()
  end

  defp results(wpms), do: Enum.map(wpms, &result(wpm: &1))

  describe "build/2" do
    test "an empty history summarises to nothing rather than crashing" do
      summary = Summary.build([])

      assert summary.tests == 0
      assert summary.best == nil
      assert summary.average_wpm == nil
      assert summary.trouble_keys == []
    end

    test "counts tests, time and words" do
      summary = Summary.build(results([60.0, 70.0, 80.0]))

      assert summary.tests == 3
      assert summary.typing_ms == 90_000
      # 175 correct characters per test, five characters to a word.
      assert summary.words_typed == 105
    end

    test "finds the best result and the averages" do
      summary = Summary.build(results([60.0, 90.0, 75.0]))

      assert summary.best.wpm == 90.0
      assert summary.average_wpm == 75.0
      assert summary.average_accuracy == 96.0
      assert summary.average_consistency == 80.0
    end

    test "averages the last ten tests separately from all time" do
      summary = Summary.build(results(List.duplicate(40.0, 10) ++ List.duplicate(80.0, 10)))

      assert summary.average_wpm == 60.0
      assert summary.recent_average == 80.0
    end

    test "improvement compares the last ten against the ten before" do
      summary = Summary.build(results(List.duplicate(50.0, 10) ++ List.duplicate(65.0, 10)))

      assert summary.improvement == 15.0
    end

    test "improvement is withheld until there is enough history to mean anything" do
      assert Summary.build(results(List.duplicate(50.0, 19))).improvement == nil
    end

    test "the trend keeps the most recent tests in order" do
      summary = Summary.build(results(Enum.map(1..30, &(&1 * 1.0))))

      assert length(summary.trend) == 20
      assert List.last(summary.trend) == 30.0
      assert hd(summary.trend) == 11.0
    end

    test "groups results by configuration, busiest first" do
      history =
        results([60.0, 80.0]) ++
          [
            result(wpm: 50.0, config: "words-25-english"),
            result(wpm: 90.0, config: "words-25-english"),
            result(wpm: 70.0, config: "words-25-english")
          ]

      assert [words, time] = Summary.build(history).by_config
      assert words.config == "words-25-english"
      assert words.tests == 3
      assert words.best == 90.0
      assert words.average == 70.0
      assert time.tests == 2
    end

    test "consistency is averaged only over the tests that measured it" do
      history = [result(consistency: nil), result(consistency: 60.0), result(consistency: 80.0)]

      assert Summary.build(history).average_consistency == 70.0
    end
  end

  describe "streaks" do
    test "counts consecutive days up to today" do
      history = [result(at: days_ago(2)), result(at: days_ago(1)), result(at: days_ago(0))]
      summary = Summary.build(history)

      assert summary.days_practiced == 3
      assert summary.streak == 3
    end

    test "several tests in one day count as one day" do
      history = [result(at: days_ago(0)), result(at: days_ago(0)), result(at: days_ago(1))]
      summary = Summary.build(history)

      assert summary.days_practiced == 2
      assert summary.streak == 2
    end

    test "a streak still counts if today's session has not happened yet" do
      history = [result(at: days_ago(2)), result(at: days_ago(1))]

      assert Summary.build(history).streak == 2
    end

    test "a gap of more than a day breaks the streak" do
      history = [result(at: days_ago(9)), result(at: days_ago(8))]

      assert Summary.build(history).streak == 0
    end

    test "only the run up to now counts, not the longest run ever" do
      history = [
        result(at: days_ago(20)),
        result(at: days_ago(19)),
        result(at: days_ago(18)),
        result(at: days_ago(1)),
        result(at: days_ago(0))
      ]

      assert Summary.build(history).streak == 2
    end
  end

  describe "per configuration lookups" do
    test "best, average and count are scoped to one configuration" do
      history =
        results([60.0, 80.0]) ++
          [result(wpm: 100.0, config: "words-25-english")]

      assert Summary.best_for(history, "time-30-english").wpm == 80.0
      assert Summary.average_for(history, "time-30-english") == 70.0
      assert Summary.count_for(history, "time-30-english") == 2
      assert Summary.best_for(history, "words-25-english").wpm == 100.0
    end

    test "a configuration never run has no best" do
      assert Summary.best_for(results([60.0]), "quote-0") == nil
      assert Summary.average_for(results([60.0]), "quote-0") == nil
      assert Summary.count_for(results([60.0]), "quote-0") == 0
    end
  end

  describe "trouble keys" do
    test "adds up letter tallies across tests" do
      history = [
        result(keys: %{"e" => %{attempts: 100, errors: 5}}),
        result(keys: %{"e" => %{attempts: 100, errors: 15}, "t" => %{attempts: 50, errors: 0}})
      ]

      assert Summary.merge_keys(history) == %{
               "e" => %{attempts: 200, errors: 20},
               "t" => %{attempts: 50, errors: 0}
             }
    end

    test "ranks the least accurate letter first" do
      keys = %{
        "e" => %{attempts: 100, errors: 20},
        "t" => %{attempts: 100, errors: 5},
        "a" => %{attempts: 100, errors: 40}
      }

      assert [worst, middle, best] = Summary.trouble_keys(keys)
      assert worst.key == "a"
      assert worst.accuracy == 60.0
      assert worst.errors == 40
      assert middle.key == "e"
      assert best.key == "t"
    end

    test "ignores letters that have barely been typed" do
      keys = %{"z" => %{attempts: 2, errors: 2}, "e" => %{attempts: 100, errors: 10}}

      assert [%{key: "e"}] = Summary.trouble_keys(keys)
    end

    test "the threshold can be lowered for a single test" do
      keys = %{"z" => %{attempts: 4, errors: 2}}

      assert [%{key: "z"}] = Summary.trouble_keys(keys, min_attempts: 3)
    end

    test "letters typed perfectly are not trouble" do
      keys = %{"e" => %{attempts: 100, errors: 0}}

      assert Summary.trouble_keys(keys) == []
    end

    test "the list is capped" do
      keys =
        Map.new(Enum.map(?a..?z, fn c -> {<<c>>, %{attempts: 100, errors: 10}} end))

      assert length(Summary.trouble_keys(keys)) == 8
      assert length(Summary.trouble_keys(keys, keys: 3)) == 3
    end
  end

  describe "slips" do
    test "adds up confusions across tests and ranks them" do
      history = [
        result(slips: %{{"e", "r"} => 10, {"n", "m"} => 2}),
        result(slips: %{{"e", "r"} => 5, {"a", "s"} => 20})
      ]

      assert [first, second, third] =
               history |> Summary.merge_slips() |> Summary.top_slips(5)

      assert first == %{expected: "a", actual: "s", count: 20}
      assert second == %{expected: "e", actual: "r", count: 15}
      assert third == %{expected: "n", actual: "m", count: 2}
    end
  end

  describe "humanize_ms/1" do
    test "reads the way a person would say it" do
      assert Summary.humanize_ms(45_000) == "45s"
      assert Summary.humanize_ms(600_000) == "10m"
      assert Summary.humanize_ms(4_320_000) == "1h 12m"
    end
  end
end
