defmodule Typr.Engine do
  @moduledoc """
  The typing test as a pure state machine.

  Every keystroke is a `key/3` call returning a new struct, and time is always
  passed in rather than read, so the whole test is deterministic and testable
  without a terminal. `Typr.App` owns the clock; this module owns the rules.

  ## Rules

    * A word is committed by pressing space, which advances to the next word
      whether or not the word was typed correctly.
    * Characters typed past the end of a word are kept as "extra" characters,
      the way monkeytype does, rather than being swallowed.
    * Backspace at the start of a word steps back to the previous word only if
      that word contains a mistake (unless `:free_backspace` is set).
    * In word and quote modes, typing the final word correctly ends the test
      without needing a trailing space.
  """

  alias Typr.Engine

  @typedoc "A per-second slice of the test, used for the results graph."
  @type sample :: %{at: float(), raw: float(), errors: non_neg_integer()}

  @type mode :: :time | :words | :quote

  @type key :: {:char, String.t()} | :space | :backspace | :backspace_word

  @type t :: %__MODULE__{}

  defstruct mode: :time,
            limit: 30,
            words: [],
            typed_rev: [],
            buf: "",
            index: 0,
            started_at: nil,
            finished_at: nil,
            keys_correct: 0,
            keys_incorrect: 0,
            spaces: 0,
            correct_spaces: 0,
            key_attempts: %{},
            key_errors: %{},
            slips: %{},
            samples_rev: [],
            sample_chars: 0,
            sample_errors: 0,
            sampled_ms: 0,
            free_backspace: false

  @doc """
  Builds a test.

  ## Options

    * `:mode` - `:time`, `:words` or `:quote` (default `:time`)
    * `:limit` - seconds for `:time` mode, otherwise the word count
    * `:words` - the target words
    * `:free_backspace` - allow returning to correctly typed words
  """
  @spec new(keyword()) :: t()
  def new(opts) do
    %Engine{
      mode: Keyword.get(opts, :mode, :time),
      limit: Keyword.get(opts, :limit, 30),
      words: Keyword.fetch!(opts, :words),
      free_backspace: Keyword.get(opts, :free_backspace, false)
    }
  end

  @doc "The words typed so far, in order, excluding the word in progress."
  @spec typed(t()) :: [String.t()]
  def typed(%Engine{typed_rev: typed_rev}), do: Enum.reverse(typed_rev)

  @doc "The word currently being typed."
  @spec current_target(t()) :: String.t()
  def current_target(%Engine{words: words, index: index}), do: Enum.at(words, index, "")

  @doc "Whether the test has started (the clock starts on the first keystroke)."
  @spec started?(t()) :: boolean()
  def started?(%Engine{started_at: started_at}), do: started_at != nil

  @doc "Whether the test is over."
  @spec finished?(t()) :: boolean()
  def finished?(%Engine{finished_at: finished_at}), do: finished_at != nil

  @doc "Milliseconds elapsed since the first keystroke."
  @spec elapsed_ms(t(), integer()) :: non_neg_integer()
  def elapsed_ms(%Engine{started_at: nil}, _now), do: 0
  def elapsed_ms(%Engine{started_at: started, finished_at: nil}, now), do: now - started
  def elapsed_ms(%Engine{started_at: started, finished_at: finished}, _now), do: finished - started

  @doc "Seconds left in a timed test, rounded up. `nil` in other modes."
  @spec remaining_seconds(t(), integer()) :: non_neg_integer() | nil
  def remaining_seconds(%Engine{mode: :time} = engine, now) do
    engine
    |> then(&(&1.limit * 1000 - elapsed_ms(&1, now)))
    |> max(0)
    |> Kernel./(1000)
    |> Float.ceil()
    |> trunc()
  end

  def remaining_seconds(_engine, _now), do: nil

  @doc "How far through the words the typist is, as `{done, total}`."
  @spec progress(t()) :: {non_neg_integer(), non_neg_integer()}
  def progress(%Engine{index: index, words: words}), do: {index, length(words)}

  @doc "True when the word supply is running low and should be topped up."
  @spec needs_words?(t(), pos_integer()) :: boolean()
  def needs_words?(%Engine{mode: :time, index: index, words: words}, lookahead) do
    length(words) - index < lookahead
  end

  def needs_words?(_engine, _lookahead), do: false

  @doc "Appends more words to an in-flight timed test."
  @spec extend(t(), [String.t()]) :: t()
  def extend(%Engine{words: words} = engine, more), do: %{engine | words: words ++ more}

  @doc """
  Applies a keystroke.

  Keystrokes after the test has finished are ignored, so a fast typist's
  trailing characters can't corrupt the result.
  """
  @spec key(t(), key(), integer()) :: t()
  def key(%Engine{finished_at: finished} = engine, _key, _now) when finished != nil, do: engine

  def key(engine, {:char, " "}, now), do: key(engine, :space, now)

  def key(engine, {:char, char}, now) do
    engine = start(engine, now)
    expected = String.at(current_target(engine), String.length(engine.buf))

    engine
    |> Map.put(:buf, engine.buf <> char)
    |> count_key(expected == char)
    |> track_letter(expected, char)
    |> maybe_finish_on_last_word(now)
  end

  def key(%Engine{buf: ""} = engine, :space, _now), do: engine
  def key(engine, :space, now), do: commit(engine, true, now)

  def key(%Engine{buf: ""} = engine, backspace, _now)
      when backspace in [:backspace, :backspace_word] do
    step_back(engine, backspace)
  end

  def key(engine, :backspace, _now) do
    %{engine | buf: String.slice(engine.buf, 0..-2//1)}
  end

  def key(engine, :backspace_word, _now), do: %{engine | buf: ""}

  @doc """
  Advances the clock.

  Closes off any whole seconds that have passed into graph samples, and ends a
  timed test once its limit is reached.
  """
  @spec tick(t(), integer()) :: t()
  def tick(%Engine{started_at: nil} = engine, _now), do: engine
  def tick(%Engine{finished_at: finished} = engine, _now) when finished != nil, do: engine

  def tick(engine, now) do
    engine = collect_samples(engine, now)

    if engine.mode == :time and elapsed_ms(engine, now) >= engine.limit * 1000 do
      finish(engine, now)
    else
      engine
    end
  end

  @doc "Ends the test early, as when the typist quits mid-run."
  @spec finish(t(), integer()) :: t()
  def finish(%Engine{started_at: nil} = engine, now), do: %{engine | started_at: now, finished_at: now}
  def finish(%Engine{finished_at: finished} = engine, _now) when finished != nil, do: engine

  def finish(engine, now) do
    # A timed test always reports exactly its limit, so a late tick can't
    # deflate the WPM by stretching the denominator.
    finished_at =
      if engine.mode == :time do
        min(now, engine.started_at + engine.limit * 1000)
      else
        now
      end

    engine
    |> collect_samples(finished_at)
    |> close_partial_sample(finished_at)
    |> Map.put(:finished_at, finished_at)
  end

  @doc "Graph samples in chronological order."
  @spec samples(t()) :: [sample()]
  def samples(%Engine{samples_rev: samples_rev}), do: Enum.reverse(samples_rev)

  @doc """
  The words paired with what was typed for them, for rendering.

  Returns maps of `:index`, `:target`, `:typed` and `:status`, where status is
  `:done`, `:current` or `:pending`.
  """
  @spec annotate(t()) :: [%{index: non_neg_integer(), target: String.t(), typed: String.t(), status: atom()}]
  def annotate(%Engine{words: words, index: index, buf: buf} = engine) do
    typed = typed(engine)

    words
    |> Enum.with_index()
    |> Enum.map(fn {target, i} ->
      cond do
        i < index -> %{index: i, target: target, typed: Enum.at(typed, i, ""), status: :done}
        i == index -> %{index: i, target: target, typed: buf, status: :current}
        true -> %{index: i, target: target, typed: "", status: :pending}
      end
    end)
  end

  defp start(%Engine{started_at: nil} = engine, now), do: %{engine | started_at: now}
  defp start(engine, _now), do: engine

  defp count_key(engine, correct?) do
    %{
      engine
      | sample_chars: engine.sample_chars + 1,
        keys_correct: engine.keys_correct + if(correct?, do: 1, else: 0),
        keys_incorrect: engine.keys_incorrect + if(correct?, do: 0, else: 1),
        sample_errors: engine.sample_errors + if(correct?, do: 0, else: 1)
    }
  end

  # Mistakes are attributed to the letter that should have been typed, which is
  # what makes them actionable: "you miss `e`" is advice, "you press `r` a lot"
  # is not. Characters typed past the end of a word have no expected letter, so
  # they are counted against accuracy but not against any key.
  defp track_letter(engine, nil, _actual), do: engine

  defp track_letter(engine, expected, actual) do
    engine = update_tally(engine, :key_attempts, expected)

    if expected == actual do
      engine
    else
      engine
      |> update_tally(:key_errors, expected)
      |> update_tally(:slips, {expected, actual})
    end
  end

  defp update_tally(engine, field, key) do
    Map.update!(engine, field, &Map.update(&1, key, 1, fn count -> count + 1 end))
  end

  # In word and quote modes the last word needs no trailing space.
  defp maybe_finish_on_last_word(%Engine{mode: :time} = engine, _now), do: engine

  defp maybe_finish_on_last_word(engine, now) do
    last? = engine.index == length(engine.words) - 1

    if last? and engine.buf == current_target(engine) do
      engine |> commit(false, now) |> finish(now)
    else
      engine
    end
  end

  defp commit(engine, space_pressed?, now) do
    correct? = engine.buf == current_target(engine)

    engine =
      if space_pressed? do
        engine
        |> count_key(correct?)
        |> Map.update!(:spaces, &(&1 + 1))
        |> Map.update!(:correct_spaces, &(&1 + if(correct?, do: 1, else: 0)))
      else
        engine
      end

    engine = %{
      engine
      | typed_rev: [engine.buf | engine.typed_rev],
        buf: "",
        index: engine.index + 1
    }

    if engine.mode != :time and engine.index >= length(engine.words) do
      finish(engine, now)
    else
      engine
    end
  end

  defp step_back(%Engine{index: 0} = engine, _backspace), do: engine

  defp step_back(engine, backspace) do
    previous_index = engine.index - 1
    [previous | rest] = engine.typed_rev

    if engine.free_backspace or previous != Enum.at(engine.words, previous_index) do
      %{
        engine
        | index: previous_index,
          typed_rev: rest,
          buf: if(backspace == :backspace_word, do: "", else: previous)
      }
    else
      engine
    end
  end

  # Closes off every whole second that has elapsed since the last sample.
  defp collect_samples(engine, now) do
    elapsed = elapsed_ms(engine, now)

    if elapsed - engine.sampled_ms >= 1000 do
      at = engine.sampled_ms + 1000

      %{
        engine
        | samples_rev: [
            %{at: at / 1000, raw: wpm_from(engine.sample_chars, 1000), errors: engine.sample_errors}
            | engine.samples_rev
          ],
          sample_chars: 0,
          sample_errors: 0,
          sampled_ms: at
      }
      |> collect_samples(now)
    else
      engine
    end
  end

  # The tail end of a test is rarely a whole second; keep it if it is long
  # enough to be meaningful, otherwise its characters are dropped from the
  # graph only (never from the score).
  defp close_partial_sample(engine, now) do
    leftover = elapsed_ms(engine, now) - engine.sampled_ms

    if leftover >= 250 and engine.sample_chars > 0 do
      %{
        engine
        | samples_rev: [
            %{
              at: (engine.sampled_ms + leftover) / 1000,
              raw: wpm_from(engine.sample_chars, leftover),
              errors: engine.sample_errors
            }
            | engine.samples_rev
          ],
          sample_chars: 0,
          sample_errors: 0
      }
    else
      engine
    end
  end

  defp wpm_from(chars, ms), do: chars / 5 * 60_000 / ms
end
