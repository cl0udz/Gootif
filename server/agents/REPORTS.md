# Gootif Reports — agent → app contract

Agents post to the **existing** notifications API (`POST /api/notifications`,
see `../API.md`). A "report" is just a notification whose `metadata` carries two
extra fields that tell the iOS app to render a rich native view instead of the
generic detail:

```jsonc
{
  "service": "premarket-brief",          // = agent id (groups it in the app)
  "title":   "Pre-Market Brief — Jun 30, 2026",
  "body":    "Risk-on into CPI; megacaps firm.",  // 1–2 line feed preview
  "priority":"medium",                   // low | medium | high
  "metadata": {
    "kind":    "news_feed",              // selects the renderer (see below)
    "payload": { /* kind-specific structured content */ }
  }
}
```

The runner builds this automatically: the agent only returns
`{ title, body, priority, payload }` and `runner.py` injects
`metadata.kind` from `agents.json`.

## Kinds & payload schemas

The iOS side decodes these (see `Gootif/Models/Report.swift`). Unknown/absent
`kind` falls back to the plain notification detail view, so nothing breaks.

| kind            | agent            | renderer            |
|-----------------|------------------|---------------------|
| `news_feed`     | premarket-brief  | Apple-News article  |
| `task_digest`   | inbox-digest     | grouped checklist   |
| `security_feed` | llm-security     | news feed cards     |

### `news_feed`
```jsonc
"payload": {
  "edition": "Pre-Market Brief",
  "summary": "markdown TL;DR",
  "items": [{
    "headline": "...", "source": "Reuters", "url": "https://...",
    "published_at": "2026-06-30T11:00:00Z",
    "summary": "what happened",
    "impact": "how it affects SPY/QQQ",
    "sentiment": "bullish|bearish|neutral",
    "tickers": ["SPY","QQQ"],
    "importance": "high|medium|low"
  }]
}
```

### `task_digest`
```jsonc
"payload": {
  "summary": "markdown overview",
  "items": [{
    "title": "...", "detail": "...",
    "category": "Needs reply|Upcoming events|Deadlines & tasks|FYI",
    "type": "email|event|task",
    "due": "2026-07-01T17:00:00Z",     // optional
    "source": "gmail: work",            // optional
    "url": "https://mail.google.com/...",// optional
    "importance": "high|medium|low"
  }]
}
```

### `security_feed`
```jsonc
"payload": {
  "summary": "markdown overview",
  "items": [{
    "headline": "...", "source": "arXiv", "url": "https://...",
    "published_at": "2026-06-28T00:00:00Z",  // optional
    "insight": "1–2 sentence distilled takeaway with the headline numbers", // required
    "innovation": "one line: the new method/technique, if any",
    "summary": "1–2 sentences of context; secondary to insight",
    "category": "LLM for security|Security of LLM",
    "venue": "academia|industry",
    "tags": ["prompt-injection","agents"],
    "importance": "high|medium|low",
    "blocks": [ /* per-item figure of the post's own numbers, when quantitative */ ]
  }]
}
```

## Visual blocks (any kind)

Every payload — and every item inside `items` — may carry an optional `blocks`
array of small typed visualizations. The app renders them natively with Swift
Charts (`Gootif/Models/ReportBlock.swift`, `Gootif/Views/Reports/BlockViews.swift`);
payload-level blocks appear after the summary, item-level blocks inside that
item's card. Unknown block types are skipped, so old app builds never break.

```jsonc
"blocks": [
  {"type": "stat_row",                     // horizontal stat tiles
   "stats": [{"label": "SPY", "value": "612.40",
              "delta": "+1.2%", "direction": "up|down|flat"}]},
  {"type": "table", "title": "optional",   // compact grid
   "columns": ["Date", "Event"], "rows": [["Jul 16", "CPI"]]},
  {"type": "bar_chart", "title": "optional", "unit": "%",
   "bars": [{"label": "Mon", "value": 1.2,
             "color": "positive|negative|neutral|accent"}]},
  {"type": "line_chart", "title": "optional", "unit": "%",  // ≤4 series
   "series": [{"name": "SPY", "points": [{"x": "Mon", "y": 0.5}]}]}
]
```

Keep blocks small: ≤8 stats, ≤10 rows, ≤12 points per series, ≤4 line series.
Agents may spawn sub-agents (Task tool, enabled in `agents.json`) to gather the
underlying data in parallel — but must only emit verified, real numbers.

## Adding a new agent
1. Add a renderer kind in `Gootif/Models/Report.swift` + a view in
   `Gootif/Views/Reports/`, and route it in `NotificationDetailView.content`.
2. Add a prompt in `prompts/` that emits `{ title, body, priority, payload }`.
3. Add an entry in `agents.json` and a cron line in `crontab.example`.
4. Optionally add a badge icon in `DesignSystem.swift` (`ServiceBadge.serviceIcons`).
