# Configuration

Set these environment variables before running. On Railway / Docker, add them in the
platform dashboard. On Cloudflare Workers, use `wrangler.toml` vars and `wrangler secret put`.

<p align="center">
  <img src="./images/linear-api-config.jpeg" width="600" alt="Linear API Configuration">
  <br>
  <sub>Webhook and API key settings in Linear's workspace settings.</sub>
</p>

## Environment Variables

| Variable | Required | Description |
| :--- | :---: | :--- |
| `LINEAR_WEBHOOK_SECRET` | Yes | HMAC-SHA256 signature verification for Linear webhooks |
| `LARK_WEBHOOK_URL` | Yes | Lark group chat webhook URL |
| `LARK_APP_ID` | Optional | Bot app ID — enables DM-on-assign |
| `LARK_APP_SECRET` | Optional | Bot app secret — pair with `LARK_APP_ID` |
| `LINEAR_API_KEY` | Optional | GraphQL API access — enables link previews |
| `LARK_VERIFICATION_TOKEN` | Optional | Lark event callback verification |
| `PORT` | No | Listen port, defaults to `3000` (ignored on CF Workers) |
| `DEBOUNCE_DELAY_MS` | No | Debounce window in ms, defaults to `30000` |

## Feature Tiers

The two required variables give you group notifications. Optional variables unlock
additional features incrementally:

1. **Base** (`LINEAR_WEBHOOK_SECRET` + `LARK_WEBHOOK_URL`) — group chat cards for issue create / update / comment.
2. **DM on assign** (+ `LARK_APP_ID` + `LARK_APP_SECRET`) — private message to the assignee when an issue is assigned.
3. **Link previews** (+ `LINEAR_API_KEY` + `LARK_VERIFICATION_TOKEN`) — paste a `linear.app` URL in Lark and it unfurls into a summary card.
