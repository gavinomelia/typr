defmodule Typr.Render do
  @moduledoc """
  Draws frames.

  Every function here is pure: given a state it returns `{iodata, caret}`,
  where caret is the screen position the terminal's own cursor should sit at.
  Using the real cursor as the caret means it blinks the way the rest of the
  terminal does, for free.

  Text is assembled as `{text, role, attributes}` segments and only turned into
  escape sequences at the last moment, so widths can be measured without
  counting invisible bytes.
  """

  alias Typr.{Engine, Stats, Terminal, Theme}

  @visible_lines 3
  @blocks ["▁", "▂", "▃", "▄", "▅", "▆", "▇"]

  @doc """
  Draws the typing screen.

  The word window scrolls so the active line sits in the middle once the
  typist is past the first line, which keeps the eye in one place.
  """
  @spec test_frame(Engine.t(), map()) :: {iodata(), {pos_integer(), pos_integer()} | nil}
  def test_frame(engine, view) do
    %{width: width, left: left, theme: theme} = view

    lines = engine |> Engine.annotate() |> layout(width)
    current_line = line_of(lines, engine.index)
    top = max(0, current_line - 1)
    visible = Enum.slice(lines, top, @visible_lines)

    first_row = words_row(view)

    body =
      visible
      |> Enum.with_index(first_row)
      |> Enum.map(fn {line, row} ->
        [Terminal.move(row, left), emit(line_segments(line), theme)]
      end)

    caret = caret_position(lines, engine, current_line, top, first_row, view)

    frame = [
      header(engine, view),
      body,
      footer(view, "tab restart · esc quit")
    ]

    {frame, caret}
  end

  @doc """
  Draws the results screen.

  Blocks are stacked in order and the whole stack is centred, so a short
  terminal simply loses the graph rather than pushing the numbers off screen.
  """
  @spec results_frame(Stats.t(), map()) :: {iodata(), nil}
  def results_frame(stats, view) do
    %{left: left, theme: theme, size: {rows, _columns}} = view

    blocks =
      [figures_block(stats, view), chart_block(stats, view), detail_block(stats, view)]
      |> Enum.concat()

    top = max(1, div(rows - length(blocks), 2))

    body =
      blocks
      |> Enum.with_index(top)
      |> Enum.map(fn {segments, row} -> [Terminal.move(row, left), emit(segments, theme)] end)

    {[body, footer(view, "tab new test · r repeat · esc quit")], nil}
  end

  defp figures_block(stats, view) do
    figures = [
      {"wpm", format_number(stats.wpm)},
      {"acc", "#{format_number(stats.accuracy)}%"},
      {"raw", format_number(stats.raw)},
      {"consistency", percent_or_dash(stats.consistency)}
    ]

    column = min(20, max(12, div(view.width, length(figures))))

    [
      Enum.map(figures, fn {label, _value} -> {String.pad_trailing(label, column), :dim, []} end),
      Enum.map(figures, fn {_label, value} -> {String.pad_trailing(value, column), :accent, [:bold]} end),
      []
    ]
  end

  # The graph is the first thing to go when there is no room for it: the
  # numbers underneath are what people actually read.
  defp chart_block(%Stats{samples: []}, _view), do: []

  defp chart_block(stats, view) do
    {rows, _columns} = view.size
    height = min(8, rows - 14)

    if height < 3 do
      []
    else
      chart(stats.samples, view.width, height) ++ [[]]
    end
  end

  defp detail_block(stats, view) do
    [
      detail_segments(stats, view),
      comparison_segments(view[:comparison]),
      best_segments(view[:best]),
      trouble_segments(view[:trouble])
    ]
    |> Enum.reject(&(&1 == nil))
  end

  defp comparison_segments(nil), do: nil
  defp comparison_segments(text), do: [{text, :dim, []}]

  defp best_segments(nil), do: nil
  defp best_segments(message), do: [{message, :accent, []}]

  defp trouble_segments(nil), do: nil
  defp trouble_segments([]), do: nil

  defp trouble_segments(keys) do
    [{"trouble ", :dim, []} | Enum.flat_map(keys, &trouble_key_segments/1)]
  end

  defp trouble_key_segments(key) do
    [
      {" #{display_key(key.key)}", :incorrect, []},
      {" #{format_number(key.accuracy)}%", :dim, []}
    ]
  end

  defp display_key(" "), do: "space"
  defp display_key(key), do: key

  @doc "Draws the message shown when the window is too small to type in."
  @spec too_small_frame(map()) :: {iodata(), nil}
  def too_small_frame(view) do
    {rows, columns} = view.size
    message = "terminal too small (#{columns}x#{rows}) — needs at least 40x8"
    {[Terminal.move(max(1, div(rows, 2)), 1), emit([{message, :incorrect, []}], view.theme)], nil}
  end

  @doc """
  Packs words into lines that fit `width`.

  A word's footprint is the longer of its target and what was typed for it, so
  a word that has grown past its length pushes its neighbours along instead of
  overflowing the column.
  """
  @spec layout([map()], pos_integer()) :: [[map()]]
  def layout(words, width) do
    words
    |> Enum.reduce({[], [], 0}, fn word, {lines, line, column} ->
      size = word_width(word)
      needed = if line == [], do: size, else: size + 1

      if column + needed > width and line != [] do
        {[Enum.reverse(line) | lines], [Map.put(word, :column, 0)], size}
      else
        start = if line == [], do: column, else: column + 1
        {lines, [Map.put(word, :column, start) | line], start + size}
      end
    end)
    |> then(fn {lines, line, _column} -> Enum.reverse([Enum.reverse(line) | lines]) end)
    |> Enum.reject(&(&1 == []))
  end

  defp word_width(%{target: target, typed: typed}) do
    max(String.length(target), String.length(typed))
  end

  defp line_of(lines, index) do
    lines
    |> Enum.find_index(fn line -> Enum.any?(line, &(&1.index == index)) end)
    |> Kernel.||(0)
  end

  defp words_row(view) do
    {rows, _columns} = view.size
    max(3, div(rows - @visible_lines, 2))
  end

  defp header(engine, view) do
    %{left: left, width: width, theme: theme} = view
    row = words_row(view) - 2

    left_text =
      case engine.mode do
        :time ->
          Integer.to_string(Engine.remaining_seconds(engine, view.now))

        _ ->
          {done, total} = Engine.progress(engine)
          "#{done}/#{total}"
      end

    right_text =
      if view.live_wpm? and Engine.started?(engine) do
        "#{format_number(Stats.live_wpm(engine, view.now))} wpm"
      else
        view.label
      end

    [
      [Terminal.move(row, left), emit([{left_text, :accent, []}], theme)],
      [
        Terminal.move(row, left + width - String.length(right_text)),
        emit([{right_text, :dim, []}], theme)
      ]
    ]
  end

  defp footer(view, text) do
    {rows, _columns} = view.size
    %{left: left, width: width, theme: theme} = view
    column = left + max(0, div(width - String.length(text), 2))

    [Terminal.move(rows - 1, column), emit([{text, :dim, []}], theme)]
  end

  # Turns one laid-out line into coloured segments, padding the gaps between
  # words so a single move sequence per line is enough.
  defp line_segments(line) do
    line
    |> Enum.reduce({[], 0}, fn word, {segments, column} ->
      gap = String.duplicate(" ", max(0, word.column - column))
      cells = word_segments(word)
      {[[{gap, :untyped, []} | cells] | segments], word.column + word_width(word)}
    end)
    |> elem(0)
    |> Enum.reverse()
    |> List.flatten()
    |> merge_segments()
  end

  defp word_segments(word) do
    target = String.graphemes(word.target)
    typed = String.graphemes(word.typed)
    attributes = if word.status == :done and word.typed != word.target, do: [:underline], else: []

    typed_part =
      target
      |> Enum.with_index()
      |> Enum.map(fn {expected, i} ->
        case Enum.at(typed, i) do
          nil -> {expected, :untyped, attributes}
          ^expected -> {expected, :correct, attributes}
          _actual -> {expected, :incorrect, attributes}
        end
      end)

    extra_part =
      typed
      |> Enum.drop(length(target))
      |> Enum.map(&{&1, :extra, attributes})

    typed_part ++ extra_part
  end

  # Collapses runs of identically styled characters into one segment.
  defp merge_segments(segments) do
    segments
    |> Enum.chunk_by(fn {_text, role, attributes} -> {role, attributes} end)
    |> Enum.map(fn chunk ->
      {_text, role, attributes} = hd(chunk)
      {Enum.map_join(chunk, "", fn {text, _, _} -> text end), role, attributes}
    end)
  end

  defp caret_position(lines, engine, current_line, top, first_row, view) do
    with true <- current_line - top < @visible_lines,
         line when is_list(line) <- Enum.at(lines, current_line),
         word when is_map(word) <- Enum.find(line, &(&1.index == engine.index)) do
      row = first_row + (current_line - top)
      column = view.left + word.column + String.length(engine.buf)
      {row, min(column, view.left + view.width)}
    else
      _ -> nil
    end
  end

  defp detail_segments(stats, view) do
    characters =
      "#{stats.correct}/#{stats.incorrect}/#{stats.extra}/#{stats.missed}"

    [
      {"chars ", :dim, []},
      {characters, :text, []},
      {"  ·  time ", :dim, []},
      {format_duration(stats.elapsed_ms), :text, []},
      {"  ·  ", :dim, []},
      {view.label, :text, []}
    ]
  end

  @doc """
  Renders per-second speed as a bar chart.

  Samples are bucketed down to the available width, so a two minute test and a
  fifteen second one both fill the same space.
  """
  @spec chart([Engine.sample()], pos_integer(), pos_integer()) :: [[tuple()]]
  def chart([], _width, _height), do: []

  def chart(samples, width, height) do
    label_width = 6
    columns = max(1, width - label_width)
    buckets = bucket(samples, columns)
    ceiling = ceiling_for(buckets)

    bars =
      for row <- 0..(height - 1) do
        level = height - row

        cells =
          Enum.map(buckets, fn bucket ->
            bar_cell(bucket.raw / ceiling * height, level)
          end)

        label =
          if row == 0,
            do: pad_left(Integer.to_string(ceiling), label_width - 1),
            else: pad_left("", label_width - 1)

        [
          {label <> " ", :dim, []},
          {Enum.join(cells), :accent, []}
        ]
      end

    axis = [
      {pad_left("0", label_width - 1) <> " ", :dim, []},
      {String.duplicate("─", length(buckets)), :dim, []}
    ]

    errors = error_row(buckets, label_width)
    time_axis = time_axis(samples, buckets, label_width)

    bars ++ [axis, errors, time_axis]
  end

  defp bar_cell(cells, level) do
    full = trunc(cells)
    fraction = cells - full

    cond do
      full >= level -> "█"
      full == level - 1 and fraction > 0.08 -> Enum.at(@blocks, min(6, trunc(fraction * 7)))
      true -> " "
    end
  end

  defp error_row(buckets, label_width) do
    marks =
      Enum.map_join(buckets, "", fn bucket ->
        if bucket.errors > 0, do: "•", else: " "
      end)

    [{pad_left("", label_width - 1) <> " ", :dim, []}, {marks, :incorrect, []}]
  end

  defp time_axis(samples, buckets, label_width) do
    finish = samples |> List.last() |> Map.fetch!(:at) |> round()
    label = "#{finish}s"
    gap = max(1, length(buckets) - String.length(label) - 1)

    [
      {pad_left("", label_width - 1) <> " ", :dim, []},
      {"0" <> String.duplicate(" ", gap) <> label, :dim, []}
    ]
  end

  # Averages samples into at most `columns` buckets, keeping any error in a
  # bucket visible rather than averaging it away.
  defp bucket(samples, columns) when length(samples) <= columns, do: samples

  defp bucket(samples, columns) do
    size = ceil(length(samples) / columns)

    samples
    |> Enum.chunk_every(size)
    |> Enum.map(fn chunk ->
      %{
        at: List.last(chunk).at,
        raw: Enum.sum(Enum.map(chunk, & &1.raw)) / length(chunk),
        errors: Enum.sum(Enum.map(chunk, & &1.errors))
      }
    end)
  end

  # Rounds the top of the scale up to something readable.
  defp ceiling_for(buckets) do
    peak = buckets |> Enum.map(& &1.raw) |> Enum.max() |> Kernel.max(1.0)
    step = if peak > 200, do: 50, else: 20
    max(step, ceil(peak / step) * step)
  end

  defp emit(segments, theme) do
    Enum.map(segments, fn {text, role, attributes} ->
      Theme.paint(theme, role, text, attributes)
    end)
  end

  defp pad_left(text, width), do: String.pad_leading(text, width)

  defp format_number(value), do: value |> Float.round(0) |> trunc() |> Integer.to_string()

  defp percent_or_dash(nil), do: "--"
  defp percent_or_dash(value), do: "#{format_number(value)}%"

  defp format_duration(ms) when ms < 60_000, do: "#{Float.round(ms / 1000, 1)}s"

  defp format_duration(ms) do
    seconds = div(ms, 1000)
    "#{div(seconds, 60)}m#{rem(seconds, 60)}s"
  end
end
