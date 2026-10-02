defmodule Hello do
  @moduledoc "Greets people."

  @doc "Says hello to `name`."
  def greet(name) when is_binary(name) do
    "Hello, #{name}! " <> Hello.Shout.loud("😀 ok")
  end
end
