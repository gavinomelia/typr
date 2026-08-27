defmodule Typr.Theme do
  @moduledoc """
  Colour roles and the SGR sequences that paint them.

  Each theme maps a role to `{basic, extended}` colour codes so the same theme
  works on an eight-colour terminal and a 256-colour one. Roles, not colours,
  are used at the call site, which keeps the renderer free of literals.
  """

  @themes %{
    "default" => %{
      text: {37, 252},
      dim: {90, 240},
      untyped: {90, 245},
      correct: {97, 231},
      incorrect: {91, 203},
      extra: {31, 131},
      accent: {93, 221}
    },
    "matrix" => %{
      text: {32, 157},
      dim: {32, 22},
      untyped: {32, 65},
      correct: {92, 46},
      incorrect: {91, 196},
      extra: {31, 88},
      accent: {92, 118}
    },
    "ocean" => %{
      text: {36, 152},
      dim: {34, 24},
      untyped: {34, 67},
      correct: {96, 195},
      incorrect: {91, 210},
      extra: {31, 95},
      accent: {96, 81}
    },
    "mono" => %{
      text: {37, 250},
      dim: {90, 238},
      untyped: {90, 243},
      correct: {97, 255},
      incorrect: {90, 240},
      extra: {90, 236},
      accent: {97, 255}
    }
  }

  @reset "\e[0m"

  @doc "Names of the available themes."
  @spec names() :: [String.t()]
  def names, do: @themes |> Map.keys() |> Enum.sort()

  @doc "Whether a theme exists."
  @spec exists?(String.t()) :: boolean()
  def exists?(name), do: Map.has_key?(@themes, name)

  @doc """
  Builds a theme's role-to-escape-sequence lookup.

  Colour depth is detected once here rather than on every painted character.
  """
  @spec build(String.t()) :: map()
  def build(name) do
    palette = Map.get(@themes, name, @themes["default"])
    extended? = extended_colour?()

    Map.new(palette, fn {role, {basic, extended}} ->
      {role, sequence(basic, extended, extended?)}
    end)
  end

  @doc "Wraps text in a role's colour."
  @spec paint(map(), atom(), iodata()) :: iodata()
  def paint(theme, role, text), do: [Map.fetch!(theme, role), text, @reset]

  @doc "Wraps text in a role's colour plus extra attributes, such as underline."
  @spec paint(map(), atom(), iodata(), [:underline | :bold | :reverse]) :: iodata()
  def paint(theme, role, text, attributes) do
    [Map.fetch!(theme, role), Enum.map(attributes, &attribute/1), text, @reset]
  end

  @doc "The reset sequence."
  @spec reset() :: String.t()
  def reset, do: @reset

  defp attribute(:underline), do: "\e[4m"
  defp attribute(:bold), do: "\e[1m"
  defp attribute(:reverse), do: "\e[7m"

  defp sequence(_basic, extended, true), do: "\e[38;5;#{extended}m"
  defp sequence(basic, _extended, false), do: "\e[#{basic}m"

  defp extended_colour? do
    term = System.get_env("TERM") || ""

    System.get_env("COLORTERM") not in [nil, ""] or
      String.contains?(term, "256") or
      String.contains?(term, "direct")
  end
end
