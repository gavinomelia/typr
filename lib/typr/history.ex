defmodule Typr.History do
  @moduledoc """
  Every completed test, appended to a file.

  Storing whole results rather than just personal bests means averages,
  trends and per-letter weaknesses can all be recomputed later, including
  statistics that had not been thought of when the result was recorded.

  The format is tab-separated so it can be read by eye, sorted, or fed to
  `awk` without this program's help. Per-letter tallies are encoded as
  codepoints (`101,240,12` = the letter `e`, attempted 240 times, missed 12)
  because the letters themselves can be punctuation that would collide with
  the separators.
  """

  alias Typr.Stats

  @header "# typr history v1\tat\tmode\tlimit\tlist\tflags\twpm\traw\taccuracy\tconsistency\tcorrect\tincorrect\textra\tmissed\tduration_ms\tkeys\tslips"

  @type result :: %{
          at: String.t(),
          mode: atom(),
          limit: integer(),
          list: String.t(),
          punctuation: boolean(),
          numbers: boolean(),
          config: String.t(),
          wpm: float(),
          raw: float(),
          accuracy: float(),
          consistency: float() | nil,
          correct: integer(),
          incorrect: integer(),
          extra: integer(),
          missed: integer(),
          duration_ms: integer(),
          keys: %{String.t() => %{attempts: integer(), errors: integer()}},
          slips: %{{String.t(), String.t()} => integer()}
        }

  @doc "Where results are stored, honouring `XDG_CONFIG_HOME`."
  @spec path() :: String.t()
  def path do
    base = System.get_env("XDG_CONFIG_HOME") || Path.join(System.user_home!(), ".config")
    Path.join([base, "typr", "history.tsv"])
  end

  @doc """
  Appends a completed test.

  Failures are swallowed: an unwritable disk should not interrupt someone
  in the middle of practising.
  """
  @spec append(Stats.t(), keyword()) :: :ok
  def append(%Stats{} = stats, opts) do
    file = path()

    with :ok <- File.mkdir_p(Path.dirname(file)),
         :ok <- ensure_header(file) do
      File.write(file, encode(stats, opts) <> "\n", [:append])
    end

    :ok
  rescue
    _ -> :ok
  end

  @doc "Every recorded result, oldest first. Unreadable lines are skipped."
  @spec load() :: [result()]
  def load do
    case File.read(path()) do
      {:ok, contents} ->
        contents
        |> String.split("\n", trim: true)
        |> Enum.reject(&String.starts_with?(&1, "#"))
        |> Enum.flat_map(&decode/1)

      {:error, _reason} ->
        []
    end
  end

  @doc "The configuration key a result is filed under, such as `time-30-english-punctuation`."
  @spec config(keyword()) :: String.t()
  def config(opts) do
    [
      to_string(opts[:mode]),
      to_string(opts[:limit]),
      if(opts[:mode] == :quote, do: nil, else: opts[:list]),
      if(opts[:punctuation], do: "punctuation", else: nil),
      if(opts[:numbers], do: "numbers", else: nil)
    ]
    |> Enum.reject(&is_nil/1)
    |> Enum.join("-")
  end

  defp ensure_header(file) do
    if File.exists?(file), do: :ok, else: File.write(file, @header <> "\n")
  end

  defp encode(stats, opts) do
    [
      timestamp(),
      to_string(opts[:mode]),
      to_string(opts[:limit]),
      opts[:list],
      encode_flags(opts),
      round2(stats.wpm),
      round2(stats.raw),
      round2(stats.accuracy),
      encode_optional(stats.consistency),
      stats.correct,
      stats.incorrect,
      stats.extra,
      stats.missed,
      stats.elapsed_ms,
      encode_keys(stats.keys),
      encode_slips(stats.slips)
    ]
    |> Enum.map_join("\t", &to_string/1)
  end

  defp decode(line) do
    case String.split(line, "\t") do
      [
        at,
        mode,
        limit,
        list,
        flags,
        wpm,
        raw,
        accuracy,
        consistency,
        correct,
        incorrect,
        extra,
        missed,
        duration,
        keys,
        slips
      ] ->
        punctuation? = String.contains?(flags, "punctuation")
        numbers? = String.contains?(flags, "numbers")
        mode = decode_mode(mode)

        result = %{
          at: at,
          mode: mode,
          limit: to_integer(limit),
          list: list,
          punctuation: punctuation?,
          numbers: numbers?,
          wpm: to_float(wpm),
          raw: to_float(raw),
          accuracy: to_float(accuracy),
          consistency: decode_optional(consistency),
          correct: to_integer(correct),
          incorrect: to_integer(incorrect),
          extra: to_integer(extra),
          missed: to_integer(missed),
          duration_ms: to_integer(duration),
          keys: decode_keys(keys),
          slips: decode_slips(slips)
        }

        [
          Map.put(
            result,
            :config,
            config(
              mode: mode,
              limit: result.limit,
              list: list,
              punctuation: punctuation?,
              numbers: numbers?
            )
          )
        ]

      _malformed ->
        []
    end
  rescue
    _ -> []
  end

  defp decode_mode("words"), do: :words
  defp decode_mode("quote"), do: :quote
  defp decode_mode(_time), do: :time

  defp encode_flags(opts) do
    [
      if(opts[:punctuation], do: "punctuation", else: nil),
      if(opts[:numbers], do: "numbers", else: nil)
    ]
    |> Enum.reject(&is_nil/1)
    |> case do
      [] -> "-"
      flags -> Enum.join(flags, ",")
    end
  end

  defp encode_keys(keys) when map_size(keys) == 0, do: "-"

  defp encode_keys(keys) do
    Enum.map_join(keys, ";", fn {key, %{attempts: attempts, errors: errors}} ->
      "#{codepoint(key)},#{attempts},#{errors}"
    end)
  end

  defp decode_keys("-"), do: %{}

  defp decode_keys(encoded) do
    encoded
    |> String.split(";", trim: true)
    |> Enum.flat_map(fn entry ->
      case String.split(entry, ",") do
        [key, attempts, errors] ->
          [{grapheme(key), %{attempts: to_integer(attempts), errors: to_integer(errors)}}]

        _ ->
          []
      end
    end)
    |> Map.new()
  end

  defp encode_slips(slips) when map_size(slips) == 0, do: "-"

  defp encode_slips(slips) do
    Enum.map_join(slips, ";", fn {{expected, actual}, count} ->
      "#{codepoint(expected)},#{codepoint(actual)},#{count}"
    end)
  end

  defp decode_slips("-"), do: %{}

  defp decode_slips(encoded) do
    encoded
    |> String.split(";", trim: true)
    |> Enum.flat_map(fn entry ->
      case String.split(entry, ",") do
        [expected, actual, count] ->
          [{{grapheme(expected), grapheme(actual)}, to_integer(count)}]

        _ ->
          []
      end
    end)
    |> Map.new()
  end

  defp codepoint(<<code::utf8>>), do: code
  defp codepoint(other), do: other |> String.to_charlist() |> List.first() || 63

  defp grapheme(code) do
    <<to_integer(code)::utf8>>
  rescue
    _ -> "?"
  end

  # Local wall-clock time rather than UTC: "days practised" and "streak" are
  # about the typist's day, and an evening session should not land on tomorrow.
  # `:calendar.local_time/0` avoids needing a timezone database.
  defp timestamp do
    :calendar.local_time() |> NaiveDateTime.from_erl!() |> NaiveDateTime.to_iso8601()
  end

  defp round2(value) when is_float(value), do: Float.round(value, 2)
  defp round2(value), do: value

  defp encode_optional(nil), do: "-"
  defp encode_optional(value), do: round2(value)

  defp decode_optional("-"), do: nil
  defp decode_optional(value), do: to_float(value)

  defp to_integer(text) do
    case Integer.parse(text) do
      {value, _rest} -> value
      :error -> 0
    end
  end

  defp to_float(text) do
    case Float.parse(text) do
      {value, _rest} -> value
      :error -> 0.0
    end
  end
end
