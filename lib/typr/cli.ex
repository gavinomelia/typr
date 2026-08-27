defmodule Typr.CLI do
  @moduledoc """
  Command line entry point.

  Parses options, sets the terminal up, and — importantly — puts it back the
  way it found it whatever happens, including on a crash.
  """

  alias Typr.{App, History, Report, Stats, Summary, Terminal, Theme, Words}

  @switches [
    time: :integer,
    words: :integer,
    quote: :boolean,
    list: :string,
    punctuation: :boolean,
    numbers: :boolean,
    theme: :string,
    width: :integer,
    seed: :integer,
    live_wpm: :boolean,
    free_backspace: :boolean,
    themes: :boolean,
    lists: :boolean,
    stats: :boolean,
    doctor: :boolean,
    version: :boolean,
    help: :boolean
  ]

  @aliases [t: :time, w: :words, q: :quote, l: :list, p: :punctuation, n: :numbers, h: :help]

  @doc false
  def main(argv) do
    case OptionParser.parse(argv, strict: @switches, aliases: @aliases) do
      {parsed, [], []} -> dispatch(parsed)
      {_parsed, [extra | _], []} -> abort("unexpected argument: #{extra}")
      {_parsed, _rest, [{flag, _} | _]} -> abort("unknown option: #{flag}")
    end
  end

  defp dispatch(parsed) do
    cond do
      parsed[:help] -> IO.puts(usage())
      parsed[:version] -> IO.puts("typr #{Typr.version()}")
      parsed[:themes] -> IO.puts(Enum.join(Theme.names(), "\n"))
      parsed[:lists] -> IO.puts(Enum.join(Words.list_names(), "\n"))
      parsed[:stats] -> print_stats()
      parsed[:doctor] -> print_diagnosis()
      true -> start(parsed)
    end
  end

  defp start(parsed) do
    seed(parsed[:seed])

    with {:ok, opts} <- build_opts(parsed),
         {:ok, stats} <- play(opts) do
      report(stats)
    else
      {:error, message} -> abort(message)
    end
  end

  # A seed makes the words reproducible, so two people can race the same test
  # and a failing screen can be reproduced exactly.
  defp seed(nil), do: :ok
  defp seed(value), do: :rand.seed(:exsss, {value, value, value})

  # Raw mode has to succeed before anything is drawn, and has to be undone
  # whatever happens afterwards — including a crash, which would otherwise
  # leave the shell with no echo.
  defp play(opts) do
    case Terminal.raw_mode() do
      {:error, _reason} ->
        {:error, "typr needs an interactive terminal"}

      {:ok, mode} ->
        Terminal.enter_screen()

        try do
          App.run(opts)
        after
          Terminal.leave_screen()
          Terminal.restore(mode)
        end
    end
  end

  defp report(nil), do: :ok

  defp report(%Stats{} = stats) do
    IO.puts(
      "#{round(stats.wpm)} wpm · #{round(stats.accuracy)}% acc · " <>
        "#{round(stats.raw)} raw · #{consistency(stats)} consistency"
    )
  end

  defp consistency(%Stats{consistency: nil}), do: "--"
  defp consistency(%Stats{consistency: value}), do: "#{round(value)}%"

  defp build_opts(parsed) do
    with {:ok, mode, limit} <- resolve_mode(parsed),
         {:ok, list} <- resolve_list(parsed),
         {:ok, theme} <- resolve_theme(parsed) do
      {:ok,
       [
         mode: mode,
         limit: limit,
         list: list,
         theme: theme,
         punctuation: parsed[:punctuation] || false,
         numbers: parsed[:numbers] || false,
         width: max(20, parsed[:width] || 72),
         live_wpm: Keyword.get(parsed, :live_wpm, true),
         free_backspace: parsed[:free_backspace] || false
       ]}
    end
  end

  defp resolve_mode(parsed) do
    cond do
      parsed[:quote] ->
        {:ok, :quote, 0}

      parsed[:words] ->
        if parsed[:words] > 0,
          do: {:ok, :words, parsed[:words]},
          else: {:error, "word count must be positive"}

      true ->
        seconds = parsed[:time] || 30
        if seconds > 0, do: {:ok, :time, seconds}, else: {:error, "time must be positive"}
    end
  end

  defp resolve_list(parsed) do
    list = parsed[:list] || "english"

    if Words.vocabulary(list) do
      {:ok, list}
    else
      {:error, "unknown word list: #{list} (try: #{Enum.join(Words.list_names(), ", ")})"}
    end
  end

  defp resolve_theme(parsed) do
    theme = parsed[:theme] || "default"

    if Theme.exists?(theme) do
      {:ok, theme}
    else
      {:error, "unknown theme: #{theme} (try: #{Enum.join(Theme.names(), ", ")})"}
    end
  end

  defp print_stats do
    History.load() |> Summary.build() |> Report.render() |> IO.write()
  end

  defp print_diagnosis do
    Enum.each(Typr.Terminal.diagnose(), fn {key, value} ->
      IO.puts("#{String.pad_trailing(to_string(key), 16)} #{value}")
    end)
  end

  defp abort(message) do
    IO.puts(:stderr, "typr: #{message}")
    System.halt(1)
  end

  defp usage do
    """
    typr — a typing test for the terminal

    usage: typr [options]

    modes
      -t, --time SECONDS      timed test (default: 30)
      -w, --words COUNT       fixed word-count test
      -q, --quote             type a sentence

    text
      -l, --list NAME         word list: #{Enum.join(Words.list_names(), ", ")}
      -p, --punctuation       mix in punctuation and capitals
      -n, --numbers           mix in numbers

    display
          --theme NAME        #{Enum.join(Theme.names(), ", ")}
          --width COLUMNS     text column width (default: 72)
          --seed N            reproducible words — same seed, same test
          --no-live-wpm       hide the live speed counter

    behaviour
          --free-backspace    allow going back to correctly typed words

    other
          --stats             show bests, averages and trouble keys
          --doctor            report terminal capabilities
          --themes            list themes
          --lists             list word lists
          --version
      -h, --help

    keys
      tab                     restart with new words
      r                       repeat the same words (results screen)
      backspace               delete a character
      ctrl+w                  delete the current word
      esc                     quit
    """
  end
end
