defmodule Typr.RenderTest do
  use ExUnit.Case, async: true

  alias Typr.{Engine, Render, Stats, Theme}

  defp view(overrides \\ []) do
    Enum.into(overrides, %{
      theme: Theme.build("default"),
      size: {24, 80},
      width: 40,
      left: 5,
      now: 0,
      live_wpm?: true,
      label: "30s · english",
      best: nil
    })
  end

  defp words(list) do
    Enum.map(Enum.with_index(list), fn {target, i} ->
      %{index: i, target: target, typed: "", status: :pending}
    end)
  end

  # Renders a frame and strips the escape sequences, leaving what a person sees.
  defp visible(frame) do
    frame
    |> IO.iodata_to_binary()
    |> String.replace(~r/\e\[[0-9;?]*[A-Za-z]/, "")
  end

  # Chart rows are still `{text, role, attributes}` segments, not iodata.
  defp segment_text(segments) do
    Enum.map_join(segments, "", fn {text, _role, _attributes} -> text end)
  end

  describe "layout/2" do
    test "breaks lines at the column width" do
      lines = Render.layout(words(~w(aaaa bbbb cccc dddd)), 10)

      assert Enum.map(lines, fn line -> Enum.map(line, & &1.target) end) == [
               ~w(aaaa bbbb),
               ~w(cccc dddd)
             ]
    end

    test "positions words with a single space between them" do
      [line] = Render.layout(words(~w(one two)), 40)

      assert Enum.map(line, & &1.column) == [0, 4]
    end

    test "a word longer than the width gets a line to itself rather than vanishing" do
      lines = Render.layout(words(~w(a supercalifragilistic b)), 10)

      assert Enum.map(lines, fn line -> Enum.map(line, & &1.target) end) == [
               ~w(a),
               ~w(supercalifragilistic),
               ~w(b)
             ]
    end

    test "overtyped words push their neighbours along" do
      typed = [
        %{index: 0, target: "ab", typed: "abcdefgh", status: :done},
        %{index: 1, target: "cd", typed: "", status: :pending}
      ]

      [line] = Render.layout(typed, 40)

      assert Enum.map(line, & &1.column) == [0, 9]
    end
  end

  describe "test_frame/2" do
    test "shows the words and puts the caret where the typist is" do
      engine = Engine.new(mode: :time, limit: 30, words: ~w(the quick brown))
      engine = Engine.key(engine, {:char, "t"}, 0)

      {frame, caret} = Render.test_frame(engine, view())
      output = visible(frame)

      assert output =~ "the quick brown"
      assert output =~ "tab restart · esc quit"
      # One character in, so the caret sits one column past the word's start.
      assert {_row, 6} = caret
    end

    test "counts down in timed mode and counts words otherwise" do
      timed = Engine.new(mode: :time, limit: 30, words: ~w(the quick))
      counted = Engine.new(mode: :words, limit: 2, words: ~w(the quick))

      assert visible(elem(Render.test_frame(timed, view(live_wpm?: false)), 0)) =~ "30"
      assert visible(elem(Render.test_frame(counted, view(live_wpm?: false)), 0)) =~ "0/2"
    end

    test "scrolls so the active line is never the last one on screen" do
      words = Enum.map(1..60, fn _ -> "acorn" end)
      engine = Engine.new(mode: :time, limit: 60, words: words)

      # Skip far enough ahead that the active word is well past the first line.
      engine = Enum.reduce(1..20, engine, fn _, acc -> commit(acc, "acorn") end)

      {_frame, {row, _column}} = Render.test_frame(engine, view())
      {rows, _columns} = view().size

      assert row < rows - 1
    end

    test "the frame changes as characters are typed" do
      engine = Engine.new(mode: :time, limit: 30, words: ~w(the quick brown))
      before = visible(elem(Render.test_frame(engine, view()), 0))
      after_typing = visible(elem(Render.test_frame(Engine.key(engine, {:char, "x"}, 0), view()), 0))

      refute before == after_typing
    end
  end

  describe "results_frame/2" do
    test "reports the headline figures" do
      engine =
        Engine.new(mode: :words, limit: 1, words: ~w(acorn))
        |> Engine.key({:char, "a"}, 0)
        |> Engine.finish(60_000)

      stats = Stats.compute(engine, 60_000)
      {frame, caret} = Render.results_frame(stats, view(best: "new personal best"))
      output = visible(frame)

      assert output =~ "wpm"
      assert output =~ "acc"
      assert output =~ "consistency"
      assert output =~ "chars"
      assert output =~ "new personal best"
      assert output =~ "tab new test · r repeat · esc quit"
      assert caret == nil
    end

    test "consistency shows a dash when there was not enough data" do
      engine =
        Engine.new(mode: :words, limit: 1, words: ~w(acorn))
        |> Engine.key({:char, "a"}, 0)
        |> Engine.finish(500)

      output = visible(elem(Render.results_frame(Stats.compute(engine, 500), view()), 0))

      assert output =~ "--"
    end
  end

  describe "chart/3" do
    test "is empty without samples" do
      assert Render.chart([], 40, 8) == []
    end

    test "draws one column per sample and marks the seconds with errors" do
      samples = [
        %{at: 1.0, raw: 60.0, errors: 0},
        %{at: 2.0, raw: 30.0, errors: 2},
        %{at: 3.0, raw: 90.0, errors: 0}
      ]

      output = samples |> Render.chart(40, 8) |> Enum.map(&segment_text/1)

      assert Enum.any?(output, &String.contains?(&1, "█"))
      assert Enum.any?(output, &String.contains?(&1, "•"))
      assert Enum.any?(output, &String.contains?(&1, "3s"))
    end

    test "buckets a long test down to the available width" do
      samples =
        Enum.map(1..120, fn second -> %{at: second * 1.0, raw: 50.0, errors: 0} end)

      [top | _] = Render.chart(samples, 40, 8)
      bars = top |> segment_text() |> String.trim()

      assert String.length(bars) <= 40
    end
  end

  defp commit(engine, word) do
    word
    |> String.graphemes()
    |> Enum.reduce(engine, fn character, acc -> Engine.key(acc, {:char, character}, 0) end)
    |> Engine.key(:space, 0)
  end
end
