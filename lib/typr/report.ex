defmodule Typr.Report do
  @moduledoc """
  Formats a `Typr.Summary` for the shell.

  Plain text rather than the ANSI the test screen uses, so the output survives
  being piped into a pager or a file.
  """

  alias Typr.Summary

  @sparkline ["▁", "▂", "▃", "▄", "▅", "▆", "▇", "█"]
  @label_width 16

  @doc "Renders a summary as the text printed by `typr --stats`."
  @spec render(Summary.t()) :: String.t()
  def render(%Summary{tests: 0}) do
    "no results yet — run typr a few times and come back\n"
  end

  def render(%Summary{} = summary) do
    [
      headline(summary),
      "",
      overview(summary),
      trend(summary),
      by_config(summary),
      trouble_keys(summary),
      slips(summary)
    ]
    |> List.flatten()
    |> Enum.join("\n")
    |> Kernel.<>("\n")
  end

  defp headline(summary) do
    "typr — #{pluralize(summary.tests, "test")} · #{Summary.humanize_ms(summary.typing_ms)} typing · #{commify(summary.words_typed)} words"
  end

  defp overview(summary) do
    [
      row("best", "#{round(summary.best.wpm)} wpm", "#{summary.best.config}, #{ago(summary.best.at)}"),
      row("average", "#{round(summary.average_wpm)} wpm"),
      row("last 10", "#{round(summary.recent_average)} wpm", improvement(summary.improvement)),
      row("accuracy", "#{percent(summary.average_accuracy)}"),
      row("consistency", "#{percent(summary.average_consistency)}"),
      row("practised", pluralize(summary.days_practiced, "day"), streak(summary.streak)),
      row("last test", ago(summary.last_at))
    ]
  end

  defp trend(%Summary{trend: trend}) when length(trend) < 2, do: []

  defp trend(%Summary{trend: trend}) do
    low = trend |> Enum.min() |> round()
    high = trend |> Enum.max() |> round()

    ["", row("recent", sparkline(trend), "#{low}–#{high} wpm")]
  end

  defp by_config(%Summary{by_config: configs}) do
    width = configs |> Enum.map(&String.length(&1.config)) |> Enum.max(fn -> 0 end)

    rows =
      Enum.map(configs, fn config ->
        "  #{String.pad_trailing(config.config, width)}  " <>
          "#{String.pad_leading(to_string(config.tests), 4)} #{if config.tests == 1, do: "test ", else: "tests"}   " <>
          "best #{String.pad_leading(to_string(round(config.best)), 3)}   " <>
          "avg #{String.pad_leading(to_string(round(config.average)), 3)}"
      end)

    ["", "by test", rows]
  end

  defp trouble_keys(%Summary{trouble_keys: []}), do: []

  defp trouble_keys(%Summary{trouble_keys: keys}) do
    rows =
      Enum.map(keys, fn key ->
        "  #{display_key(key.key)}   #{String.pad_leading(percent(key.accuracy), 6)}   " <>
          "#{commify(key.errors)} missed of #{commify(key.attempts)}"
      end)

    ["", "trouble keys", rows]
  end

  defp slips(%Summary{slips: []}), do: []

  defp slips(%Summary{slips: slips}) do
    rows =
      Enum.map(slips, fn slip ->
        "  #{display_key(slip.expected)} → #{display_key(slip.actual)}   #{commify(slip.count)}"
      end)

    ["", "most often typed instead", rows]
  end

  @doc """
  Draws a sequence of values as a one-line sparkline.

  The scale spans the values themselves rather than starting at zero, so small
  differences between good scores stay visible.
  """
  @spec sparkline([number()]) :: String.t()
  def sparkline([]), do: ""

  def sparkline(values) do
    low = Enum.min(values)
    high = Enum.max(values)
    span = high - low

    Enum.map_join(values, "", fn value ->
      position = if span == 0, do: 0.5, else: (value - low) / span
      Enum.at(@sparkline, min(7, trunc(position * 8)))
    end)
  end

  defp row(label, value, note \\ nil)
  defp row(label, value, nil), do: "  #{String.pad_trailing(label, @label_width)}#{value}"

  defp row(label, value, note) do
    "  #{String.pad_trailing(label, @label_width)}#{String.pad_trailing(value, 10)}#{note}"
  end

  defp improvement(nil), do: nil
  defp improvement(delta) when delta >= 0, do: "+#{round(delta)} on the 10 before"
  defp improvement(delta), do: "#{round(delta)} on the 10 before"

  defp streak(0), do: nil
  defp streak(1), do: "typed today"
  defp streak(days), do: "#{days} day streak"

  # Space and punctuation need naming, or the column silently swallows them.
  defp display_key(" "), do: "space"
  defp display_key(key), do: key

  defp percent(nil), do: "--"
  defp percent(value), do: "#{Float.round(value, 1)}%"

  defp ago(nil), do: "never"

  defp ago(at) do
    with {:ok, naive} <- NaiveDateTime.from_iso8601(at),
         today = :calendar.local_time() |> elem(0) |> Date.from_erl!(),
         days when is_integer(days) <- Date.diff(today, NaiveDateTime.to_date(naive)) do
      case days do
        0 -> "today"
        1 -> "yesterday"
        days when days < 30 -> "#{days} days ago"
        days -> "#{div(days, 30)} months ago"
      end
    else
      _ -> at
    end
  end

  defp pluralize(1, noun), do: "1 #{noun}"
  defp pluralize(count, noun), do: "#{commify(count)} #{noun}s"

  defp commify(number) do
    number
    |> to_string()
    |> String.reverse()
    |> String.replace(~r/(\d{3})(?=\d)/, "\\1,")
    |> String.reverse()
  end
end
