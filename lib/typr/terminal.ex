defmodule Typr.Terminal do
  @moduledoc """
  Raw-mode terminal handling and ANSI output.

  Reading one keystroke at a time takes some care on the BEAM. The runtime
  hands its children pipes rather than the terminal, and it has no controlling
  terminal of its own, so neither `stty` on stdin nor `stty` on `/dev/tty`
  reaches the right device. What does work is asking the operating system which
  terminal the runtime is attached to — `ps -o tty=` — and pointing `stty` at
  that device by name.

  OTP 28 offers `shell:start_interactive({noshell, raw})` for this, and it is
  used when available. On earlier releases that call is not merely unsupported
  but harmful: the option is unrecognised, so it starts a full interactive
  shell which then eats every keystroke. Hence the version gate rather than a
  hopeful attempt.
  """

  @esc "\e"
  @device_key {__MODULE__, :device}

  # A bar cursor sits in the gap at the left edge of the cell, so the letter
  # you are about to type stays readable. A block cursor covers it.
  @caret_bar "\e[5 q"
  @caret_default "\e[0 q"

  @typedoc "How raw mode was obtained, and therefore how to undo it."
  @type mode :: :otp | {:stty, String.t(), String.t()}

  @doc "Puts the terminal into raw mode, returning how to restore it later."
  @spec raw_mode() :: {:ok, mode()} | {:error, term()}
  def raw_mode do
    use_utf8()

    case controlling_tty() do
      {:ok, device} -> stty_raw_mode(device)
      {:error, reason} -> otp_raw_mode_if_supported(reason)
    end
  end

  @doc "Undoes `raw_mode/0`."
  @spec restore(mode()) :: :ok
  def restore({:stty, device, saved}) do
    stty(device, [saved])
    :ok
  end

  def restore(:otp) do
    :shell.start_interactive({:noshell, :cooked})
    :ok
  rescue
    _ -> :ok
  catch
    :exit, _ -> :ok
  end

  @doc """
  Terminal size as `{rows, columns}`, falling back to a sane default.

  Only meaningful once `raw_mode/0` has run, since that is when the terminal
  device is discovered.
  """
  @spec size() :: {pos_integer(), pos_integer()}
  def size do
    with device when is_binary(device) <- :persistent_term.get(@device_key, nil),
         {:ok, output} <- stty(device, ["size"]),
         [rows, columns] <- String.split(String.trim(output), " "),
         {rows, ""} <- Integer.parse(rows),
         {columns, ""} <- Integer.parse(columns),
         true <- rows > 0 and columns > 0 do
      {rows, columns}
    else
      _ -> runtime_size()
    end
  end

  @doc """
  Starts a process that forwards each character from stdin to `parent` as
  `{:key, character}`, and `:input_closed` when the stream ends.

  Reading blocks, so it has to live in its own process: the main loop needs to
  keep waking on its own timer to advance the clock.
  """
  @spec start_reader(pid()) :: pid()
  def start_reader(parent) do
    spawn_link(fn -> read_loop(parent) end)
  end

  @doc """
  Collects keystrokes for a while and reports what arrived.

  Lets the diagnostics tell "the terminal is not in raw mode" apart from "the
  keys are arriving but are being misread".
  """
  @spec probe_input(pos_integer()) :: [binary()]
  def probe_input(duration_ms) do
    reader = start_reader(self())
    deadline = System.monotonic_time(:millisecond) + duration_ms

    keys = collect_keys(deadline, [])
    Process.unlink(reader)
    Process.exit(reader, :kill)
    keys
  end

  @doc """
  Reports what the terminal layer can and cannot do here.

  Terminal handling is the part of this program most likely to behave
  differently on someone else's machine, so it can be interrogated directly
  rather than guessed at from a failure message.
  """
  @spec diagnose() :: keyword()
  def diagnose do
    raw = raw_mode()

    report = [
      otp_release: Integer.to_string(otp_release()),
      term: System.get_env("TERM") || "(unset)",
      colorterm: System.get_env("COLORTERM") || "(unset)",
      controlling_tty: inspect(controlling_tty()),
      raw_mode: inspect(raw),
      size: inspect(size()),
      input_2s: inspect(probe_input(2_000))
    ]

    with {:ok, mode} <- raw, do: restore(mode)
    report
  end

  @doc "Switches to the alternate screen buffer and hides the cursor."
  @spec enter_screen() :: :ok
  def enter_screen do
    write([@esc, "[?1049h", @esc, "[?25l", @esc, "[2J", @caret_bar])
  end

  @doc "Restores the primary screen buffer, the cursor and its shape."
  @spec leave_screen() :: :ok
  def leave_screen do
    write([@esc, "[?25h", @caret_default, @esc, "[?1049l", @esc, "[0m"])
  end

  @doc """
  Paints a frame.

  The frame is wrapped in synchronized-output markers so terminals that support
  them present it in one piece instead of tearing mid-repaint; terminals that
  don't simply ignore the sequence.
  """
  @spec paint(iodata(), {pos_integer(), pos_integer()} | nil) :: :ok
  def paint(frame, caret) do
    write([
      @esc,
      "[?2026h",
      @esc,
      "[H",
      @esc,
      "[2J",
      frame,
      caret_sequence(caret),
      @esc,
      "[?2026l"
    ])
  end

  @doc "Moves the cursor to a 1-indexed row and column."
  @spec move(pos_integer(), pos_integer()) :: iodata()
  def move(row, column), do: [@esc, "[", Integer.to_string(row), ";", Integer.to_string(column), "H"]

  @doc """
  Writes to stdout.

  `IO.write/1` rather than `IO.binwrite/1`: the latter tags its data as latin1,
  so the runtime re-encodes every byte of a multi-byte character and the block
  characters in the results graph come out as mojibake.
  """
  @spec write(iodata()) :: :ok
  def write(iodata), do: IO.write(iodata)

  # The runtime defaults standard_io to latin1 in some environments, which
  # would mangle the drawing characters on the way out.
  defp use_utf8 do
    :io.setopts(:standard_io, encoding: :unicode)
  rescue
    _ -> :ok
  catch
    _, _ -> :ok
  end

  defp caret_sequence(nil), do: [@esc, "[?25l"]
  defp caret_sequence({row, column}), do: [move(row, column), @esc, "[?25h"]

  defp read_loop(parent) do
    case IO.getn(:stdio, "", 1) do
      :eof ->
        send(parent, :input_closed)

      {:error, _reason} ->
        send(parent, :input_closed)

      character ->
        send(parent, {:key, character})
        read_loop(parent)
    end
  end

  defp collect_keys(deadline, acc) do
    remaining = deadline - System.monotonic_time(:millisecond)

    if remaining <= 0 do
      Enum.reverse(acc)
    else
      receive do
        {:key, character} -> collect_keys(deadline, [character | acc])
        :input_closed -> Enum.reverse(acc)
      after
        remaining -> Enum.reverse(acc)
      end
    end
  end

  defp stty_raw_mode(device) do
    with {:ok, saved} <- stty(device, ["-g"]),
         {:ok, _output} <- stty(device, ["raw", "-echo"]) do
      :persistent_term.put(@device_key, device)
      {:ok, {:stty, device, String.trim(saved)}}
    end
  end

  # Asks the operating system which terminal this process is attached to. `ps`
  # prints a bare device name such as `ttys006` or `pts/3`, or `?` when there
  # is no terminal at all.
  defp controlling_tty do
    case System.cmd("ps", ["-o", "tty=", "-p", List.to_string(:os.getpid())], stderr_to_stdout: true) do
      {output, 0} ->
        case String.trim(output) do
          name when name in ["", "?", "??", "-"] -> {:error, :no_controlling_terminal}
          name -> {:ok, Path.join("/dev", name)}
        end

      {_output, _status} ->
        {:error, :no_controlling_terminal}
    end
  rescue
    error -> {:error, error}
  end

  defp otp_raw_mode_if_supported(reason) do
    if otp_release() >= 28 do
      case :shell.start_interactive({:noshell, :raw}) do
        :ok -> {:ok, :otp}
        _error -> {:error, reason}
      end
    else
      {:error, reason}
    end
  rescue
    _ -> {:error, reason}
  catch
    :exit, _ -> {:error, reason}
  end

  defp runtime_size do
    case {:io.rows(), :io.columns()} do
      {{:ok, rows}, {:ok, columns}} when rows > 0 and columns > 0 -> {rows, columns}
      _ -> {24, 80}
    end
  end

  defp stty(device, args) do
    case System.cmd("stty", [device_flag(), device | args], stderr_to_stdout: true) do
      {output, 0} -> {:ok, output}
      {output, status} -> {:error, {status, String.trim(output)}}
    end
  rescue
    error -> {:error, error}
  end

  # BSD stty spells the device flag `-f`; GNU stty spells it `-F`.
  defp device_flag do
    case :os.type() do
      {:unix, os} when os in [:darwin, :freebsd, :openbsd, :netbsd] -> "-f"
      _ -> "-F"
    end
  end

  defp otp_release do
    :erlang.system_info(:otp_release) |> List.to_string() |> String.to_integer()
  rescue
    _ -> 0
  end
end
