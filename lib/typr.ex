defmodule Typr do
  @moduledoc """
  A typing test for the terminal, in the spirit of monkeytype.

  The interesting parts are `Typr.Engine`, which models a test as a pure state
  machine, and `Typr.Stats`, which scores one. Neither knows a terminal exists.
  """

  @version Mix.Project.config()[:version]

  @doc "The current version."
  @spec version() :: String.t()
  def version, do: @version
end
