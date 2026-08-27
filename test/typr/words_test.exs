defmodule Typr.WordsTest do
  use ExUnit.Case, async: true

  alias Typr.Words

  describe "generate/3" do
    test "produces the requested number of words" do
      assert length(Words.generate("english", 25)) == 25
    end

    test "never repeats a word back to back" do
      words = Words.generate("english", 500)

      refute words |> Enum.chunk_every(2, 1, :discard) |> Enum.any?(fn [a, b] -> a == b end)
    end

    test "draws only from the requested vocabulary" do
      vocabulary = MapSet.new(Words.vocabulary("english_extended"))

      assert Enum.all?(Words.generate("english_extended", 100), &MapSet.member?(vocabulary, &1))
    end

    test "falls back to the default vocabulary for an unknown list" do
      assert length(Words.generate("klingon", 10)) == 10
    end
  end

  describe "punctuation" do
    test "capitalises the opening word" do
      for _ <- 1..20 do
        [first | _] = Words.generate("english", 10, punctuation: true)
        assert first =~ ~r/^[("'\[]?[A-Z]/
      end
    end

    test "adds marks that plain generation never produces" do
      plain = Words.generate("english", 300) |> Enum.join(" ")
      punctuated = Words.generate("english", 300, punctuation: true) |> Enum.join(" ")

      refute plain =~ ~r/[.,;:!?]/
      assert punctuated =~ ~r/[.,;:!?]/
    end

    test "a word after a full stop starts a new sentence" do
      words = Words.generate("english", 400, punctuation: true)

      words
      |> Enum.chunk_every(2, 1, :discard)
      |> Enum.filter(fn [previous, _next] -> String.ends_with?(previous, [".", "!", "?"]) end)
      |> Enum.each(fn [_previous, next] -> assert next =~ ~r/^[("'\[]?[A-Z]/ end)
    end
  end

  describe "numbers" do
    test "mixes digits in when asked, and never otherwise" do
      assert Enum.join(Words.generate("english", 400, numbers: true)) =~ ~r/\d/
      refute Enum.join(Words.generate("english", 400)) =~ ~r/\d/
    end
  end

  describe "quote_words/0" do
    test "returns a sentence as separate words" do
      words = Words.quote_words()

      assert length(words) > 5
      refute Enum.any?(words, &String.contains?(&1, " "))
    end
  end

  describe "vocabulary/1" do
    test "every listed name resolves" do
      assert Enum.all?(Words.list_names(), &(Words.vocabulary(&1) != nil))
    end

    test "unknown names do not" do
      assert Words.vocabulary("nope") == nil
    end

    test "lists hold no duplicates or stray whitespace" do
      for name <- Words.list_names() do
        vocabulary = Words.vocabulary(name)

        assert length(Enum.uniq(vocabulary)) == length(vocabulary), "#{name} has duplicates"
        assert Enum.all?(vocabulary, &(&1 == String.trim(&1)))
        assert Enum.all?(vocabulary, &(&1 != ""))
      end
    end
  end
end
