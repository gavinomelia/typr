defmodule Typr.EngineTest do
  use ExUnit.Case, async: true

  alias Typr.Engine

  defp engine(opts \\ []) do
    opts
    |> Keyword.put_new(:words, ~w(the quick brown fox))
    |> Engine.new()
  end

  defp type(engine, text, now \\ 0) do
    text
    |> String.graphemes()
    |> Enum.reduce(engine, fn character, acc -> Engine.key(acc, {:char, character}, now) end)
  end

  describe "typing" do
    test "the clock starts on the first keystroke, not on creation" do
      engine = engine()
      refute Engine.started?(engine)

      engine = type(engine, "t", 1_000)
      assert Engine.started?(engine)
      assert Engine.elapsed_ms(engine, 3_000) == 2_000
    end

    test "space commits a word and moves to the next" do
      engine = engine() |> type("the") |> Engine.key(:space, 0)

      assert engine.index == 1
      assert Engine.typed(engine) == ["the"]
      assert engine.buf == ""
      assert Engine.current_target(engine) == "quick"
    end

    test "space is ignored before anything has been typed" do
      engine = Engine.key(engine(), :space, 0)

      assert engine.index == 0
      refute Engine.started?(engine)
    end

    test "characters typed past the end of a word are kept as extras" do
      engine = type(engine(), "theee")

      assert engine.buf == "theee"
      assert engine.keys_correct == 3
      assert engine.keys_incorrect == 2
    end

    test "a word can be committed wrong and left behind" do
      engine = engine() |> type("teh") |> Engine.key(:space, 0)

      assert Engine.typed(engine) == ["teh"]
      assert engine.index == 1
      assert engine.correct_spaces == 0
      assert engine.spaces == 1
    end

    test "accuracy is judged at the moment a key is pressed" do
      engine = engine() |> type("th") |> Engine.key(:backspace, 0) |> type("e")

      # The typo was corrected, but it still happened.
      assert engine.buf == "te"
      assert engine.keys_correct == 2
      assert engine.keys_incorrect == 1
    end
  end

  describe "backspace" do
    test "deletes one character at a time" do
      engine = engine() |> type("the") |> Engine.key(:backspace, 0)

      assert engine.buf == "th"
    end

    test "ctrl+w clears the whole word" do
      engine = engine() |> type("the") |> Engine.key(:backspace_word, 0)

      assert engine.buf == ""
      assert engine.index == 0
    end

    test "steps back into a previous word that has a mistake" do
      engine =
        engine()
        |> type("teh")
        |> Engine.key(:space, 0)
        |> Engine.key(:backspace, 0)

      assert engine.index == 0
      assert engine.buf == "teh"
      assert Engine.typed(engine) == []
    end

    test "refuses to step back into a correctly typed word" do
      engine =
        engine()
        |> type("the")
        |> Engine.key(:space, 0)
        |> Engine.key(:backspace, 0)

      assert engine.index == 1
      assert engine.buf == ""
    end

    test "free_backspace allows stepping back into a correct word" do
      engine =
        engine(free_backspace: true)
        |> type("the")
        |> Engine.key(:space, 0)
        |> Engine.key(:backspace, 0)

      assert engine.index == 0
      assert engine.buf == "the"
    end

    test "does nothing at the very start" do
      engine = Engine.key(engine(), :backspace, 0)

      assert engine.index == 0
      assert engine.buf == ""
    end
  end

  describe "finishing" do
    test "word mode ends when the last word is typed correctly, without a space" do
      engine =
        engine(mode: :words, limit: 4, words: ~w(a b c d))
        |> type("a")
        |> Engine.key(:space, 0)
        |> type("b")
        |> Engine.key(:space, 0)
        |> type("c")
        |> Engine.key(:space, 0)
        |> type("d")

      assert Engine.finished?(engine)
      assert Engine.typed(engine) == ~w(a b c d)
    end

    test "word mode ends on a space after the last word even when it is wrong" do
      engine =
        engine(mode: :words, limit: 1, words: ~w(alpha))
        |> type("alpga")
        |> Engine.key(:space, 0)

      assert Engine.finished?(engine)
    end

    test "timed mode ends once the limit passes" do
      engine = engine(mode: :time, limit: 30) |> type("the", 0)

      refute Engine.finished?(Engine.tick(engine, 29_999))

      engine = Engine.tick(engine, 30_100)
      assert Engine.finished?(engine)
    end

    test "a timed test reports exactly its limit even if the tick lands late" do
      engine =
        engine(mode: :time, limit: 15)
        |> type("the", 0)
        |> Engine.tick(15_400)

      assert Engine.elapsed_ms(engine, 20_000) == 15_000
    end

    test "keystrokes after the end are ignored" do
      engine =
        engine(mode: :time, limit: 5)
        |> type("the", 0)
        |> Engine.tick(5_000)
        |> type("xxxx", 5_100)

      assert engine.buf == "the"
      assert engine.keys_incorrect == 0
    end
  end

  describe "word supply" do
    test "a timed test asks for more words as it runs low" do
      engine = engine(mode: :time, limit: 60)

      assert Engine.needs_words?(engine, 10)
      refute Engine.needs_words?(engine, 3)

      engine = Engine.extend(engine, ~w(five six seven eight))
      assert length(engine.words) == 8
    end

    test "fixed-length tests never ask for more" do
      refute Engine.needs_words?(engine(mode: :words, limit: 4), 100)
    end
  end

  describe "samples" do
    test "one sample is recorded per elapsed second" do
      engine =
        engine(mode: :time, limit: 60)
        |> type("the", 0)
        |> Engine.tick(3_200)

      samples = Engine.samples(engine)

      assert length(samples) == 3
      assert Enum.map(samples, & &1.at) == [1.0, 2.0, 3.0]
      # Three characters landed in the first second: 3/5 of a word in 1/60 min.
      assert hd(samples).raw == 36.0
    end

    test "errors are attributed to the second they happened in" do
      engine =
        engine(mode: :time, limit: 60)
        |> type("teh", 0)
        |> Engine.tick(1_000)

      assert [%{errors: 2}] = Engine.samples(engine)
    end
  end

  describe "annotate" do
    test "labels every word by how far the typist has got" do
      engine = engine() |> type("the") |> Engine.key(:space, 0) |> type("qu")

      assert [
               %{index: 0, target: "the", typed: "the", status: :done},
               %{index: 1, target: "quick", typed: "qu", status: :current},
               %{index: 2, target: "brown", typed: "", status: :pending},
               %{index: 3, target: "fox", typed: "", status: :pending}
             ] = Engine.annotate(engine)
    end
  end

  describe "per-letter tracking" do
    test "counts every letter that was attempted" do
      engine = type(engine(), "the")

      assert engine.key_attempts == %{"t" => 1, "h" => 1, "e" => 1}
      assert engine.key_errors == %{}
      assert engine.slips == %{}
    end

    test "blames the letter that should have been typed, not the one that was" do
      engine = type(engine(), "tje")

      assert engine.key_attempts == %{"t" => 1, "h" => 1, "e" => 1}
      assert engine.key_errors == %{"h" => 1}
      assert engine.slips == %{{"h", "j"} => 1}
    end

    test "tallies repeats of the same mistake" do
      engine =
        engine(words: ~w(the the))
        |> type("tje")
        |> Engine.key(:space, 0)
        |> type("tje")

      assert engine.key_errors == %{"h" => 2}
      assert engine.slips == %{{"h", "j"} => 2}
    end

    test "characters typed past the end of a word are blamed on no letter" do
      engine = type(engine(), "theee")

      assert engine.key_attempts == %{"t" => 1, "h" => 1, "e" => 1}
      assert engine.key_errors == %{}
      # The extras still count against accuracy, they just have no owner.
      assert engine.keys_incorrect == 2
    end

    test "a corrected letter keeps its mistake on the record" do
      engine = engine() |> type("tj") |> Engine.key(:backspace, 0) |> type("h")

      assert engine.key_attempts == %{"t" => 1, "h" => 2}
      assert engine.key_errors == %{"h" => 1}
    end
  end

  describe "remaining_seconds" do
    test "counts down and stops at zero" do
      engine = engine(mode: :time, limit: 30) |> type("t", 0)

      assert Engine.remaining_seconds(engine, 0) == 30
      assert Engine.remaining_seconds(engine, 500) == 30
      assert Engine.remaining_seconds(engine, 29_500) == 1
      assert Engine.remaining_seconds(engine, 30_000) == 0
    end

    test "is undefined outside timed mode" do
      assert Engine.remaining_seconds(engine(mode: :words), 0) == nil
    end
  end
end
