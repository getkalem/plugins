defmodule HelloTest do
  use ExUnit.Case

  test "greets" do
    assert Hello.greet("x") =~ "Hello"
  end
end
