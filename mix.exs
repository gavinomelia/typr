defmodule Typr.MixProject do
  use Mix.Project

  def project do
    [
      app: :typr,
      version: "1.0.0",
      elixir: "~> 1.14",
      start_permanent: Mix.env() == :prod,
      escript: escript(),
      deps: deps()
    ]
  end

  def application do
    [extra_applications: []]
  end

  defp escript do
    [main_module: Typr.CLI, name: "typr"]
  end

  defp deps do
    []
  end
end
