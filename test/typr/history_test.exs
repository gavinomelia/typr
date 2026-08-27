defmodule Typr.HistoryTest do
  # Not async: these tests point XDG_CONFIG_HOME at a temporary directory.
  use ExUnit.Case, async: false

  alias Typr.{History, Stats}

  setup do
    directory = Path.join(System.tmp_dir!(), "typr-test-#{System.unique_integer([:positive])}")
    previous = System.get_env("XDG_CONFIG_HOME")
    System.put_env("XDG_CONFIG_HOME", directory)

    on_exit(fn ->
      File.rm_rf(directory)
      if previous, do: System.put_env("XDG_CONFIG_HOME", previous), else: System.delete_env("XDG_CONFIG_HOME")
    end)

    :ok
  end

  defp stats(overrides \\ %{}) do
    Map.merge(
      %Stats{
        wpm: 72.5,
        raw: 78.1,
        accuracy: 96.4,
        consistency: 81.2,
        correct: 210,
        incorrect: 6,
        extra: 1,
        missed: 2,
        elapsed_ms: 30_000,
        keys: %{"e" => %{attempts: 40, errors: 3}, "t" => %{attempts: 30, errors: 0}},
        slips: %{{"e", "r"} => 3}
      },
      overrides
    )
  end

  defp opts(overrides \\ []) do
    Keyword.merge(
      [mode: :time, limit: 30, list: "english", punctuation: false, numbers: false],
      overrides
    )
  end

  describe "load/0" do
    test "is empty when nothing has been recorded" do
      assert History.load() == []
    end
  end

  describe "append/2 and load/0" do
    test "a result survives the round trip" do
      History.append(stats(), opts())

      assert [result] = History.load()
      assert result.wpm == 72.5
      assert result.accuracy == 96.4
      assert result.consistency == 81.2
      assert result.correct == 210
      assert result.duration_ms == 30_000
      assert result.mode == :time
      assert result.limit == 30
      assert result.list == "english"
      assert result.config == "time-30-english"
      assert result.keys == %{"e" => %{attempts: 40, errors: 3}, "t" => %{attempts: 30, errors: 0}}
      assert result.slips == %{{"e", "r"} => 3}
    end

    test "results accumulate in the order they were run" do
      History.append(stats(%{wpm: 60.0}), opts())
      History.append(stats(%{wpm: 70.0}), opts())
      History.append(stats(%{wpm: 65.0}), opts())

      assert Enum.map(History.load(), & &1.wpm) == [60.0, 70.0, 65.0]
    end

    test "flags become part of the configuration key" do
      History.append(stats(), opts(punctuation: true, numbers: true))

      assert [result] = History.load()
      assert result.punctuation
      assert result.numbers
      assert result.config == "time-30-english-punctuation-numbers"
    end

    test "a missing consistency stays missing rather than becoming zero" do
      History.append(stats(%{consistency: nil}), opts())

      assert [%{consistency: nil}] = History.load()
    end

    test "punctuation characters in key tallies do not corrupt the row" do
      # Semicolons and commas are the separators used inside the keys column,
      # so they are exactly the characters that could break the encoding.
      keys = %{";" => %{attempts: 12, errors: 4}, "," => %{attempts: 9, errors: 1}}
      slips = %{{";", ","} => 4, {" ", "n"} => 2}

      History.append(stats(%{keys: keys, slips: slips}), opts(punctuation: true))

      assert [result] = History.load()
      assert result.keys == keys
      assert result.slips == slips
    end

    test "the file is human readable, with a header and one row per test" do
      History.append(stats(), opts())
      History.append(stats(), opts())

      lines = History.path() |> File.read!() |> String.split("\n", trim: true)

      assert [header | rows] = lines
      assert String.starts_with?(header, "# typr history v1")
      assert length(rows) == 2
      assert Enum.all?(rows, &(length(String.split(&1, "\t")) == 16))
    end

    test "corrupt lines are skipped instead of taking the file down with them" do
      History.append(stats(%{wpm: 60.0}), opts())
      File.write(History.path(), "this is not a result\n", [:append])
      History.append(stats(%{wpm: 70.0}), opts())

      assert Enum.map(History.load(), & &1.wpm) == [60.0, 70.0]
    end
  end

  describe "config/1" do
    test "names each combination distinctly" do
      assert History.config(opts()) == "time-30-english"
      assert History.config(opts(mode: :words, limit: 25)) == "words-25-english"
      assert History.config(opts(punctuation: true)) == "time-30-english-punctuation"
      assert History.config(opts(list: "english_extended")) == "time-30-english_extended"
    end

    test "quote mode has no word list to name" do
      assert History.config(opts(mode: :quote, limit: 0)) == "quote-0"
    end
  end
end
