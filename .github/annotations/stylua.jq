# Turns stylua's --output-format=json lines, one per unformatted file,
# into GitHub annotations. Lines in the report are 0-based.
# Usage: stylua --check --output-format=json lua | jq -r -f stylua.jq

def prop: gsub("%"; "%25") | gsub(":"; "%3A") | gsub(","; "%2C");

.file as $file
| .mismatches[]
| "::error file=\($file | prop),line=\(.original_start_line + 1),endLine=\(.original_end_line + 1),title=stylua::not formatted; run stylua lua"
