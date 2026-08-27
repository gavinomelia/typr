defmodule Typr.App do
  @moduledoc """
  The event loop.

  A reader process blocks on stdin and forwards each character to the loop,
  which is then free to wake on its own timer to advance the clock. That split
  is the whole reason this is pleasant to write in Elixir: no polling, no
  non-blocking reads, just a mailbox with a timeout.
  """

  alias Typr.{Engine, History, Render, Stats, Summary, Terminal, Theme, Words}

  @lookahead 40
  @initial_words 60
  @running_frame_ms 50
  @idle_frame_ms 200
  @size_interval_ms 500
  @min_columns 40
  @min_rows 8

  defstruct [
    :opts,
    :engine,
    :theme,
    :size,
    :size_checked_at,
    :stats,
    :best,
    :comparison,
    :trouble,
    screen: :test,
    last_frame: nil
  ]

  @doc """
  Runs tests until the typist quits.

  Returns the last completed result so the caller can echo it into the scroll
  back after the alternate screen is torn down.
  """
  @spec run(keyword()) :: {:ok, Stats.t() | nil}
  def run(opts) do
    state = %__MODULE__{
      opts: opts,
      engine: new_engine(opts),
      theme: Theme.build(opts[:theme]),
      size: Terminal.size(),
      size_checked_at: now()
    }

    Terminal.start_reader(self())
    loop(state)
  end

  defp loop(state) do
    state =
      state
      |> refresh_size()
      |> advance()
      |> draw()

    receive do
      {:key, character} ->
        case handle(state, classify(character)) do
          {:cont, state} -> loop(state)
          :quit -> {:ok, state.stats}
        end

      :input_closed ->
        {:ok, state.stats}
    after
      frame_interval(state) -> loop(state)
    end
  end

  # Advances the clock, tops up the word supply, and moves to the results
  # screen the moment the engine says the test is over.
  defp advance(%{screen: :results} = state), do: state

  defp advance(state) do
    engine = Engine.tick(state.engine, now())

    engine =
      if Engine.needs_words?(engine, @lookahead) do
        Engine.extend(engine, generate(state.opts, @initial_words))
      else
        engine
      end

    if Engine.finished?(engine) do
      to_results(%{state | engine: engine})
    else
      %{state | engine: engine}
    end
  end

  defp to_results(state) do
    stats = Stats.compute(state.engine, now())
    {best, comparison} = record(stats, state.opts)

    %{
      state
      | screen: :results,
        stats: stats,
        best: best,
        comparison: comparison,
        trouble: Summary.trouble_keys(stats.keys, min_attempts: 3, keys: 4),
        last_frame: nil
    }
  end

  # A run abandoned after a couple of keystrokes is noise; recording it would
  # drag every average down for no reason.
  defp record(stats, opts) do
    if stats.elapsed_ms >= 1_000 and stats.correct > 0 do
      config = History.config(opts)
      previous = Summary.best_for(History.load(), config)
      History.append(stats, opts)

      {best_message(stats, previous), comparison(History.load(), config)}
    else
      {nil, nil}
    end
  end

  defp best_message(_stats, nil), do: "first result for this test — the bar is set"

  defp best_message(stats, previous) do
    if stats.wpm > previous.wpm do
      "new personal best · +#{round(stats.wpm - previous.wpm)} wpm on #{round(previous.wpm)}"
    end
  end

  defp comparison(history, config) do
    with best when not is_nil(best) <- Summary.best_for(history, config),
         average when not is_nil(average) <- Summary.average_for(history, config) do
      count = Summary.count_for(history, config)
      "pb #{round(best.wpm)} · avg #{round(average)} · #{count} #{if count == 1, do: "test", else: "tests"}"
    else
      _ -> nil
    end
  end

  defp handle(state, key) do
    case {state.screen, key} do
      {_, :escape} -> :quit
      {:results, {:char, "q"}} -> :quit
      {:test, :tab} -> {:cont, restart(state, :new)}
      {:results, :tab} -> {:cont, restart(state, :new)}
      {:results, :enter} -> {:cont, restart(state, :new)}
      {:results, {:char, "r"}} -> {:cont, restart(state, :repeat)}
      {:test, {:char, _} = key} -> {:cont, type(state, key)}
      {:test, backspace} when backspace in [:backspace, :backspace_word] -> {:cont, type(state, backspace)}
      _ -> {:cont, state}
    end
  end

  defp type(state, key) do
    %{state | engine: Engine.key(state.engine, key, now())}
  end

  defp restart(state, :new), do: reset(state, new_engine(state.opts))
  defp restart(state, :repeat), do: reset(state, build_engine(state.opts, state.engine.words))

  defp reset(state, engine) do
    %{
      state
      | engine: engine,
        screen: :test,
        stats: nil,
        best: nil,
        comparison: nil,
        trouble: nil,
        last_frame: nil
    }
  end

  defp draw(state) do
    {frame, caret} = frame_for(state)
    binary = IO.iodata_to_binary(frame)

    if binary == state.last_frame do
      state
    else
      Terminal.paint(binary, caret)
      %{state | last_frame: binary}
    end
  end

  defp frame_for(state) do
    view = view(state)
    {rows, columns} = state.size

    cond do
      columns < @min_columns or rows < @min_rows -> Render.too_small_frame(view)
      state.screen == :results -> Render.results_frame(state.stats, view)
      true -> Render.test_frame(state.engine, view)
    end
  end

  defp view(state) do
    {rows, columns} = state.size
    width = min(state.opts[:width], columns - 4)
    left = max(1, div(columns - width, 2) + 1)

    %{
      theme: state.theme,
      size: {rows, columns},
      width: width,
      left: left,
      now: now(),
      live_wpm?: state.opts[:live_wpm],
      label: label(state.opts),
      best: state.best,
      comparison: state.comparison,
      trouble: state.trouble
    }
  end

  defp label(opts) do
    [
      case opts[:mode] do
        :time -> "#{opts[:limit]}s"
        :words -> "#{opts[:limit]} words"
        :quote -> "quote"
      end,
      if(opts[:mode] == :quote, do: nil, else: opts[:list]),
      if(opts[:punctuation], do: "punctuation", else: nil),
      if(opts[:numbers], do: "numbers", else: nil)
    ]
    |> Enum.reject(&is_nil/1)
    |> Enum.join(" · ")
  end

  # Size is polled rather than pushed: Erlang cannot subscribe to SIGWINCH, and
  # shelling out to stty on every frame would be wasteful.
  defp refresh_size(state) do
    if now() - state.size_checked_at >= @size_interval_ms do
      size = Terminal.size()
      last_frame = if size == state.size, do: state.last_frame, else: nil
      %{state | size: size, size_checked_at: now(), last_frame: last_frame}
    else
      state
    end
  end

  defp frame_interval(%{screen: :test, engine: engine}) do
    if Engine.started?(engine), do: @running_frame_ms, else: @idle_frame_ms
  end

  defp frame_interval(_state), do: @idle_frame_ms

  defp new_engine(opts), do: build_engine(opts, generate_initial(opts))

  defp build_engine(opts, words) do
    Engine.new(
      mode: opts[:mode],
      limit: opts[:limit],
      words: words,
      free_backspace: opts[:free_backspace]
    )
  end

  defp generate_initial(opts) do
    case opts[:mode] do
      :quote -> Words.quote_words()
      :time -> generate(opts, @initial_words)
      :words -> generate(opts, opts[:limit])
    end
  end

  defp generate(opts, count) do
    Words.generate(opts[:list], count, punctuation: opts[:punctuation], numbers: opts[:numbers])
  end

  defp now, do: System.monotonic_time(:millisecond)

  @doc false
  # Maps a raw character to an action. Escape sequences arrive one byte at a
  # time, so a lone escape is told apart from an arrow key by waiting briefly
  # for a follow-up that never comes.
  def classify("\e"), do: resolve_escape()
  def classify("\t"), do: :tab
  def classify(enter) when enter in ["\r", "\n"], do: :enter
  def classify(<<127>>), do: :backspace
  def classify(<<8>>), do: :backspace_word
  def classify(<<23>>), do: :backspace_word
  def classify(<<3>>), do: :quit
  def classify(<<4>>), do: :quit
  def classify(<<byte>>) when byte < 32, do: :ignore
  def classify(character), do: {:char, character}

  defp resolve_escape do
    receive do
      {:key, "["} -> drain_sequence()
      {:key, "O"} -> drain_sequence()
      {:key, _other} -> :escape
    after
      30 -> :escape
    end
  end

  # Swallows the rest of a control sequence up to its final byte.
  defp drain_sequence do
    receive do
      {:key, <<byte>>} when byte in ?@..?~ -> :ignore
      {:key, _parameter} -> drain_sequence()
    after
      30 -> :ignore
    end
  end
end
