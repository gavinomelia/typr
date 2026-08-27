defmodule Typr.Stats do
  @moduledoc """
  Scoring, following monkeytype's definitions.

    * **wpm** - correctly typed characters (plus the spaces after correctly
      typed words) divided by five, per minute. Mistyped words earn nothing,
      which is why accuracy and speed are not independent.
    * **raw** - the same figure ignoring correctness, i.e. how fast the fingers
      moved.
    * **accuracy** - correct keystrokes as a share of all keystrokes, judged at
      the moment each key was pressed. Fixing a typo does not restore accuracy.
    * **consistency** - how even the per-second raw speed was, derived from the
      coefficient of variation. 100% is a metronome.
  """

  alias Typr.Engine

  @type t :: %__MODULE__{}

  defstruct wpm: 0.0,
            raw: 0.0,
            accuracy: 0.0,
            consistency: nil,
            correct: 0,
            incorrect: 0,
            extra: 0,
            missed: 0,
            elapsed_ms: 0,
            samples: [],
            keys: %{},
            slips: %{}

  @doc "Scores a test."
  @spec compute(Engine.t(), integer()) :: t()
  def compute(engine, now) do
    elapsed_ms = Engine.elapsed_ms(engine, now)
    counts = character_counts(engine)
    minutes = elapsed_ms / 60_000
    samples = Engine.samples(engine)

    %__MODULE__{
      wpm: per_minute(counts.correct + engine.correct_spaces, minutes),
      raw: per_minute(counts.correct + counts.incorrect + counts.extra + engine.spaces, minutes),
      accuracy: accuracy(engine),
      consistency: consistency(samples),
      correct: counts.correct,
      incorrect: counts.incorrect,
      extra: counts.extra,
      missed: counts.missed,
      elapsed_ms: elapsed_ms,
      samples: samples,
      keys: key_tallies(engine),
      slips: engine.slips
    }
  end

  @doc """
  Per-letter attempts and mistakes, keyed by the letter that should have been
  typed.

  Kept as a plain map so it can be merged across tests without knowing anything
  about how it was collected.
  """
  @spec key_tallies(Engine.t()) :: %{String.t() => %{attempts: non_neg_integer(), errors: non_neg_integer()}}
  def key_tallies(%Engine{key_attempts: attempts, key_errors: errors}) do
    Map.new(attempts, fn {key, count} ->
      {key, %{attempts: count, errors: Map.get(errors, key, 0)}}
    end)
  end

  @doc """
  Compares a typed word against its target.

  Returns counts of characters that were `:correct`, `:incorrect` (typed in
  place of a different letter), `:extra` (typed past the end of the word) and
  `:missed` (letters the word had that were never typed).
  """
  @spec compare(String.t(), String.t()) :: %{
          correct: non_neg_integer(),
          incorrect: non_neg_integer(),
          extra: non_neg_integer(),
          missed: non_neg_integer()
        }
  def compare(target, typed) do
    target_chars = String.graphemes(target)
    typed_chars = String.graphemes(typed)
    overlap = min(length(target_chars), length(typed_chars))

    correct =
      target_chars
      |> Enum.zip(typed_chars)
      |> Enum.count(fn {expected, actual} -> expected == actual end)

    %{
      correct: correct,
      incorrect: overlap - correct,
      extra: max(0, length(typed_chars) - length(target_chars)),
      missed: max(0, length(target_chars) - length(typed_chars))
    }
  end

  @doc "A live WPM estimate for the header during a run."
  @spec live_wpm(Engine.t(), integer()) :: float()
  def live_wpm(engine, now) do
    case Engine.elapsed_ms(engine, now) do
      elapsed when elapsed < 1000 -> 0.0
      elapsed -> per_minute(character_counts(engine).correct + engine.correct_spaces, elapsed / 60_000)
    end
  end

  # Committed words contribute missed characters; the word still being typed
  # does not, since the typist has not abandoned it yet.
  defp character_counts(engine) do
    committed =
      engine
      |> Engine.typed()
      |> Enum.with_index()
      |> Enum.reduce(%{correct: 0, incorrect: 0, extra: 0, missed: 0}, fn {typed, i}, acc ->
        add(acc, compare(Enum.at(engine.words, i, ""), typed))
      end)

    case engine.buf do
      "" ->
        committed

      buf ->
        in_progress = %{compare(Engine.current_target(engine), buf) | missed: 0}
        add(committed, in_progress)
    end
  end

  defp add(a, b) do
    Map.new(a, fn {key, value} -> {key, value + Map.fetch!(b, key)} end)
  end

  defp per_minute(_chars, minutes) when minutes <= 0, do: 0.0
  defp per_minute(chars, minutes), do: chars / 5 / minutes

  defp accuracy(%Engine{keys_correct: 0, keys_incorrect: 0}), do: 0.0

  defp accuracy(%Engine{keys_correct: correct, keys_incorrect: incorrect}) do
    correct / (correct + incorrect) * 100
  end

  defp consistency(samples) when length(samples) < 2, do: nil

  defp consistency(samples) do
    speeds = Enum.map(samples, & &1.raw)
    mean = Enum.sum(speeds) / length(speeds)

    if mean <= 0 do
      nil
    else
      variance = Enum.sum(Enum.map(speeds, &((&1 - mean) * (&1 - mean)))) / length(speeds)
      kogasa(:math.sqrt(variance) / mean)
    end
  end

  # monkeytype's scaling of the coefficient of variation into a friendly 0-100
  # figure: an odd-power series fed through tanh, so small wobbles barely cost
  # anything while a stop-start run falls away quickly.
  defp kogasa(cov) do
    100 * (1 - :math.tanh(cov + :math.pow(cov, 3) / 3 + :math.pow(cov, 5) / 5))
  end
end
