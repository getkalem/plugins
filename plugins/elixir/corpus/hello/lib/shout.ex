defmodule Hello.Shout do
  @moduledoc "Loud words."

  @doc "Upper-cases `text`."
  def loud(text), do: String.upcase(text)
end
