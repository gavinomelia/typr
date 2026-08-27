defmodule Typr.ReportTest do
  use ExUnit.Case, async: true

  alias Typr.{Report, Summary}

  defp result(overrides) do
    Map.merge(
      %{
        at: NaiveDateTime.to_iso8601(NaiveDateTime.from_erl!(:calendar.local_time())),
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

  describe "render/1" do
    test "says so plainly when there is nothing to report" do
      assert Report.render(Summary.build([])) =~ "no results yet"
    end

    test "reports the headline figures" do
      history = [result(wpm: 60.0), result(wpm: 90.0), result(wpm: 72.0)]
      output = Report.render(Summary.build(history))

      assert output =~ "3 tests"
      assert output =~ "best"
      assert output =~ "90 wpm"
      assert output =~ "average"
      assert output =~ "74 wpm"
      assert output =~ "accuracy"
      assert output =~ "96.0%"
      assert output =~ "time-30-english"
    end

    test "lists trouble keys and slips when there are any" do
      history = [
        result(
          keys: %{"e" => %{attempts: 100, errors: 12}, "t" => %{attempts: 100, errors: 0}},
          slips: %{{"e", "r"} => 12}
        )
      ]

      output = Report.render(Summary.build(history))

      assert output =~ "trouble keys"
      assert output =~ "88.0%"
      assert output =~ "12 missed of 100"
      assert output =~ "most often typed instead"
      assert output =~ "e → r"
    end

    test "leaves out sections that have nothing in them" do
      output = Report.render(Summary.build([result([])]))

      refute output =~ "trouble keys"
      refute output =~ "most often typed instead"
    end

    test "names the space bar rather than printing a blank" do
      history = [
        result(keys: %{" " => %{attempts: 200, errors: 20}}, slips: %{{" ", "n"} => 20})
      ]

      output = Report.render(Summary.build(history))

      assert output =~ "space"
      assert output =~ "space → n"
    end

    test "groups long numbers so they can be read at a glance" do
      history = Enum.map(1..40, fn _ -> result(correct: 5_000) end)

      assert Report.render(Summary.build(history)) =~ "40,000 words"
    end
  end

  describe "sparkline/1" do
    test "is empty for no values" do
      assert Report.sparkline([]) == ""
    end

    test "draws one mark per value, lowest to highest" do
      line = Report.sparkline([10, 20, 30, 40])

      assert String.length(line) == 4
      assert String.first(line) == "▁"
      assert String.last(line) == "█"
    end

    test "a flat run sits in the middle rather than dividing by zero" do
      assert Report.sparkline([50, 50, 50]) == "▅▅▅"
    end
  end
end
