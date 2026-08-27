defmodule Typr.Summary do
  @moduledoc """
  Aggregates a history of results into the figures worth looking at.

  Pure functions over a list of result maps, so the interesting arithmetic —
  averages, trends, which letters let you down — can be tested without any
  files or terminals involved.
  """

  alias Typr.History

  @recent_window 10
  @trend_window 20

  @type t :: %__MODULE__{}

  defstruct tests: 0,
            typing_ms: 0,
            words_typed: 0,
            best: nil,
            average_wpm: nil,
            average_accuracy: nil,
            average_consistency: nil,
            recent_average: nil,
            improvement: nil,
            trend: [],
            by_config: [],
            days_practiced: 0,
            streak: 0,
            last_at: nil,
            trouble_keys: [],
            slips: []

  @doc """
  Builds a summary of every result given.

  ## Options

    * `:min_attempts` - how many times a letter must have been typed before it
      can be called a trouble key (default 20)
    * `:keys` - how many trouble keys to report (default 8)
    * `:slips` - how many letter confusions to report (default 6)
  """
  @spec build([History.result()], keyword()) :: t()
  def build(results, opts \\ [])

  def build([], _opts), do: %__MODULE__{}

  def build(results, opts) do
    wpms = Enum.map(results, & &1.wpm)
    recent = Enum.take(wpms, -@recent_window)

    %__MODULE__{
      tests: length(results),
      typing_ms: results |> Enum.map(& &1.duration_ms) |> Enum.sum(),
      words_typed: round(Enum.sum(Enum.map(results, & &1.correct)) / 5),
      best: Enum.max_by(results, & &1.wpm),
      average_wpm: average(wpms),
      average_accuracy: average(Enum.map(results, & &1.accuracy)),
      average_consistency: results |> Enum.map(& &1.consistency) |> Enum.reject(&is_nil/1) |> average(),
      recent_average: average(recent),
      improvement: improvement(wpms),
      trend: Enum.take(wpms, -@trend_window),
      by_config: by_config(results),
      days_practiced: results |> days() |> length(),
      streak: results |> days() |> streak(),
      last_at: List.last(results).at,
      trouble_keys: results |> merge_keys() |> trouble_keys(opts),
      slips: results |> merge_slips() |> top_slips(Keyword.get(opts, :slips, 6))
    }
  end

  @doc "The best result for one configuration, or `nil` if it has never been run."
  @spec best_for([History.result()], String.t()) :: History.result() | nil
  def best_for(results, config) do
    results
    |> Enum.filter(&(&1.config == config))
    |> case do
      [] -> nil
      matching -> Enum.max_by(matching, & &1.wpm)
    end
  end

  @doc "Average speed for one configuration, or `nil`."
  @spec average_for([History.result()], String.t()) :: float() | nil
  def average_for(results, config) do
    results
    |> Enum.filter(&(&1.config == config))
    |> Enum.map(& &1.wpm)
    |> average()
  end

  @doc "How many tests have been run with one configuration."
  @spec count_for([History.result()], String.t()) :: non_neg_integer()
  def count_for(results, config), do: Enum.count(results, &(&1.config == config))

  @doc "Adds up per-letter tallies across every result."
  @spec merge_keys([History.result()]) :: %{String.t() => %{attempts: integer(), errors: integer()}}
  def merge_keys(results) do
    Enum.reduce(results, %{}, fn result, acc ->
      Map.merge(acc, result.keys, fn _key, a, b ->
        %{attempts: a.attempts + b.attempts, errors: a.errors + b.errors}
      end)
    end)
  end

  @doc "Adds up letter confusions across every result."
  @spec merge_slips([History.result()]) :: %{{String.t(), String.t()} => integer()}
  def merge_slips(results) do
    Enum.reduce(results, %{}, fn result, acc ->
      Map.merge(acc, result.slips, fn _pair, a, b -> a + b end)
    end)
  end

  @doc """
  The letters that go wrong most often, worst accuracy first.

  Letters typed only a handful of times are excluded: one slip on a letter you
  have typed twice says nothing, and would otherwise dominate the list.
  """
  @spec trouble_keys(%{String.t() => map()}, keyword()) :: [map()]
  def trouble_keys(keys, opts \\ []) do
    minimum = Keyword.get(opts, :min_attempts, 20)
    limit = Keyword.get(opts, :keys, 8)

    keys
    |> Enum.filter(fn {_key, tally} -> tally.attempts >= minimum and tally.errors > 0 end)
    |> Enum.map(fn {key, tally} ->
      %{
        key: key,
        attempts: tally.attempts,
        errors: tally.errors,
        accuracy: (tally.attempts - tally.errors) / tally.attempts * 100
      }
    end)
    |> Enum.sort_by(&{&1.accuracy, -&1.errors})
    |> Enum.take(limit)
  end

  @doc "The most frequent letter confusions, as `%{expected:, actual:, count:}`."
  @spec top_slips(%{{String.t(), String.t()} => integer()}, pos_integer()) :: [map()]
  def top_slips(slips, limit) do
    slips
    |> Enum.map(fn {{expected, actual}, count} ->
      %{expected: expected, actual: actual, count: count}
    end)
    |> Enum.sort_by(&(-&1.count))
    |> Enum.take(limit)
  end

  @doc "Formats a duration the way a person would say it."
  @spec humanize_ms(integer()) :: String.t()
  def humanize_ms(ms) when ms < 60_000, do: "#{div(ms, 1000)}s"

  def humanize_ms(ms) when ms < 3_600_000, do: "#{div(ms, 60_000)}m"

  def humanize_ms(ms) do
    hours = div(ms, 3_600_000)
    minutes = div(rem(ms, 3_600_000), 60_000)
    "#{hours}h #{minutes}m"
  end

  defp by_config(results) do
    results
    |> Enum.group_by(& &1.config)
    |> Enum.map(fn {config, matching} ->
      %{
        config: config,
        tests: length(matching),
        best: matching |> Enum.map(& &1.wpm) |> Enum.max(),
        average: matching |> Enum.map(& &1.wpm) |> average()
      }
    end)
    |> Enum.sort_by(&(-&1.tests))
  end

  # Compares the most recent tests against the ones before them, which is a
  # fairer read on progress than first-ever versus latest.
  defp improvement(wpms) when length(wpms) < 2 * @recent_window, do: nil

  defp improvement(wpms) do
    recent = Enum.take(wpms, -@recent_window)
    earlier = wpms |> Enum.drop(-@recent_window) |> Enum.take(-@recent_window)

    average(recent) - average(earlier)
  end

  defp days(results) do
    results
    |> Enum.map(&date_of/1)
    |> Enum.reject(&is_nil/1)
    |> Enum.uniq()
    |> Enum.sort({:desc, Date})
  end

  # A streak is only alive if it reaches today or yesterday; otherwise it was
  # broken and the count would be a flattering lie.
  defp streak([]), do: 0

  defp streak([most_recent | _] = days) do
    if Date.diff(today(), most_recent) > 1 do
      0
    else
      count_consecutive(days, most_recent, 1)
    end
  end

  defp count_consecutive([_last], _previous, count), do: count

  defp count_consecutive([previous, next | rest], previous, count) do
    if Date.diff(previous, next) == 1 do
      count_consecutive([next | rest], next, count + 1)
    else
      count
    end
  end

  defp count_consecutive(_days, _previous, count), do: count

  # Results are timestamped in local time, so "today" has to be local too.
  defp today, do: :calendar.local_time() |> elem(0) |> Date.from_erl!()

  defp date_of(%{at: at}) do
    case NaiveDateTime.from_iso8601(at) do
      {:ok, naive} -> NaiveDateTime.to_date(naive)
      {:error, _reason} -> nil
    end
  end

  defp average([]), do: nil
  defp average(values), do: Enum.sum(values) / length(values)
end
