# Turns lua-language-server's --check_format=json report into GitHub
# annotations. Lines and columns in the report are 0-based.
# Usage: jq -r --arg root "file://$PWD/" -f luals.jq check.json

def esc: gsub("%"; "%25") | gsub("\r"; "%0D") | gsub("\n"; "%0A");
def prop: esc | gsub(":"; "%3A") | gsub(","; "%2C");

to_entries[]
| (.key | ltrimstr($root)) as $file
| .value[]
| ({"1": "error", "2": "warning"}[.severity | tostring] // "notice") as $level
| "::\($level) file=\($file | prop),line=\(.range.start.line + 1),endLine=\(.range.end.line + 1),col=\(.range.start.character + 1),title=\(.code // "lua-language-server" | prop)::\(.message | esc)"
