# Turns cargo's --message-format=json stream back into the text cargo prints,
# plus a GitHub annotation for each diagnostic and each test panic.
# Lines that are not cargo JSON, such as test output, pass through as is.
# $warning is the annotation level for compiler warnings.
# Usage: cargo ... --message-format=json | jq -jRn --arg warning W -f cargo.jq

def esc: gsub("%"; "%25") | gsub("\r"; "%0D") | gsub("\n"; "%0A");
def prop: esc | gsub(":"; "%3A") | gsub(","; "%2C");
def command($level; $props; $message):
  "::\($level)"
  + ($props | map(select(. != null)) | join(",") | if . == "" then "" else " " + . end)
  + "::\($message | esc)\n";

# a span inside a macro from outside the repo, such as format! in the
# toolchain, is walked back to the call site that expanded it
def site:
  if (.file_name | startswith("/")) and .expansion then .expansion.span | site
  else .
  end;

def diagnostic:
  .message as $m
  | ($m.spans | map(select(.is_primary)) | first | if . then site else . end) as $s
  | ({error: "error", warning: $warning}[$m.level]) as $level
  | ($m.rendered // ""),
    if $level == null then empty
    else command($level; [
      (if $s then "file=\($s.file_name | prop),line=\($s.line_start),endLine=\($s.line_end),col=\($s.column_start)" else null end),
      (if $m.code then "title=\($m.code.code | prop)" else null end)
    ]; $m.message)
    end;

def panic:
  capture("^thread '(?<test>[^']+)'( \\([0-9]+\\))? panicked at (?<file>[^:]+):(?<line>[0-9]+):(?<col>[0-9]+):")
  | command("error"; ["file=\(.file | prop),line=\(.line),col=\(.col)", "title=test failed"]; "\(.test) panicked");

# cargo repeats a diagnostic for each target that shares the file, such as
# lib and lib test; the rendered text tells repeats apart
foreach (inputs
    | . as $line
    | (try fromjson catch null)
    | if type == "object" and .reason then . else {reason: "text", text: $line} end
  ) as $e (
  {seen: {}, out: null};
  if $e.reason == "text" then .out = $e
  elif $e.reason != "compiler-message" then .out = null
  elif .seen[$e.message.rendered // ""] then .out = null
  else .seen[$e.message.rendered // ""] = true | .out = $e
  end;
  .out // empty
)
| if .reason == "text" then .text + "\n", (.text | panic) else diagnostic end
