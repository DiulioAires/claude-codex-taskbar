# Claude usage details

The taskbar strip shows Claude's overall 5-hour and weekly usage. Hover over the
strip to see the available usage windows and reset times.

## Weekly limits by model

When Claude returns a weekly limit for Opus or Sonnet, the tooltip includes a
separate line for that model. Each line shows its usage percentage and reset time,
when available. A model's weekly limit can be reached before the overall weekly
limit, so check these lines before starting work with that model.

Model windows that are missing or `null` are omitted, rather than displayed as
`0%`. Values above 100% are displayed as 100%, like the overall usage windows.
Expired model windows are removed until a new reading is available.

## Extra usage

If Claude reports that extra usage is enabled, the tooltip says so. The tooltip
does not infer a cost, balance, or spending limit from that flag. No extra-usage
line appears when the flag is disabled, missing, or `null`.

## Freshness and privacy

Model details follow the provider's existing freshness rules. If the app keeps a
previous reading after a temporary fetch failure, the tooltip retains its stale
indicator; those values are not presented as a fresh reading. Error states do not
become `0%`.

These details use the existing Claude usage response. They require no additional
request, credential access, or settings. Labels follow the Windows UI language:
Portuguese maps to Brazilian Portuguese; other languages use English.
