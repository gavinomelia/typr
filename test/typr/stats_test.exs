defmodule Typr.StatsTest do
  use ExUnit.Case, async: true

  alias Typr.{Engine, Stats}

  # Spreads the keystrokes evenly across `duration`, first key at zero and last
  # key on the buzzer, so the elapsed time is exactly the duration asked for.
  defp type(engine, text, duration) do
    graphemes = String.graphemes(text)
    last = max(1, length(graphemes) - 1)

    graphemes
    |> Enum.with_index()
    |> Enum.reduce(engine, fn {character, i}, acc ->
      now = round(duration * i / last)

      case character do
        " " -> Engine.key(acc, :space, now)
        character -> Engine.key(acc, {:char, character}, now)
      end
    end)
  end

  describe "compare/2" do
    test "counts a perfect word" do
      assert Stats.compare("brown", "brown") == %{correct: 5, incorrect: 0, extra: 0, missed: 0}
    end

    test "counts a transposition as two wrong characters" do
      assert Stats.compare("the", "teh") == %{correct: 1, incorrect: 2, extra: 0, missed: 0}
    end

    test "counts characters typed past the end of the word" do
      assert Stats.compare("the", "thee") == %{correct: 3, incorrect: 0, extra: 1, missed: 0}
    end

    test "counts characters the typist never got to" do
      assert Stats.compare("brown", "br") == %{correct: 2, incorrect: 0, extra: 0, missed: 3}
    end
  end

  describe "wpm" do
    test "a perfect minute of five-character words scores its word count" do
      words = List.duplicate("acorn", 20)

      engine =
        Engine.new(mode: :words, limit: 20, words: words)
        |> type(Enum.join(words, " "), 60_000)

      stats = Stats.compute(engine, 60_000)

      # 20 words of five characters, plus the 19 spaces between them.
      assert stats.correct == 100
      assert_in_delta stats.wpm, 23.8, 0.1
      assert stats.accuracy == 100.0
    end

    test "a mistyped word earns nothing for its wrong characters but raw counts them" do
      engine =
        Engine.new(mode: :words, limit: 2, words: ~w(acorn acorn))
        |> type("acorn acxrn", 60_000)
        |> Engine.finish(60_000)

      stats = Stats.compute(engine, 60_000)

      assert stats.correct == 9
      assert stats.incorrect == 1
      assert stats.wpm < stats.raw
      assert_in_delta stats.accuracy, 90.9, 0.1
    end

    test "an unfinished word still contributes the characters that were typed" do
      engine =
        Engine.new(mode: :time, limit: 60, words: ~w(acorn acorn))
        |> type("acorn ac", 60_000)
        |> Engine.finish(60_000)

      stats = Stats.compute(engine, 60_000)

      assert stats.correct == 7
      # The word in progress has not been abandoned, so nothing is "missed".
      assert stats.missed == 0
    end

    test "skipping a word early counts the rest of it as missed" do
      engine =
        Engine.new(mode: :words, limit: 2, words: ~w(acorn acorn))
        |> type("ac acorn", 60_000)

      stats = Stats.compute(engine, 60_000)

      assert stats.missed == 3
    end

    test "speed scales with the clock" do
      words = List.duplicate("acorn", 10)
      typing = Enum.join(words, " ")

      minute =
        Engine.new(mode: :words, limit: 10, words: words) |> type(typing, 60_000)

      half_minute =
        Engine.new(mode: :words, limit: 10, words: words) |> type(typing, 30_000)

      assert_in_delta Stats.compute(half_minute, 30_000).wpm,
                      Stats.compute(minute, 60_000).wpm * 2,
                      0.001
    end
  end

  describe "consistency" do
    test "is undefined until there are at least two samples" do
      engine =
        Engine.new(mode: :time, limit: 60, words: ~w(acorn))
        |> type("acorn", 900)
        |> Engine.finish(900)

      assert Stats.compute(engine, 900).consistency == nil
    end

    test "an even pace scores higher than a stop-start one" do
      steady = samples([40, 40, 40, 40])
      erratic = samples([10, 70, 5, 75])

      assert consistency_of(steady) > 99
      assert consistency_of(erratic) < consistency_of(steady)
    end
  end

  describe "live_wpm/2" do
    test "stays at zero for the first second so it does not spike" do
      engine = Engine.new(mode: :time, limit: 60, words: ~w(acorn)) |> type("ac", 0)

      assert Stats.live_wpm(engine, 200) == 0.0
      assert Stats.live_wpm(engine, 2_000) > 0
    end
  end

  defp samples(speeds) do
    speeds
    |> Enum.with_index(1)
    |> Enum.map(fn {raw, second} -> %{at: second * 1.0, raw: raw * 1.0, errors: 0} end)
  end

  # Consistency is derived from the samples alone, so a hand-built engine is
  # enough to exercise it.
  defp consistency_of(samples) do
    engine = %Engine{
      mode: :time,
      limit: 60,
      words: ~w(acorn),
      started_at: 0,
      finished_at: 60_000,
      samples_rev: Enum.reverse(samples),
      keys_correct: 1
    }

    Stats.compute(engine, 60_000).consistency
  end
end
