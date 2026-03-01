# Deploy to Railway (Docker)

The repo includes a multi-stage `Dockerfile` optimized for [Railway](https://railway.app/).

## Steps

1. Create a new project on Railway and connect this repository.
2. Add environment variables in the Railway dashboard. See [Configuration](./configuration.md)
   for the full list.

<p align="center">
  <img src="./images/railway-vars.png" width="600" alt="Railway Variables Configuration">
  <br>
  <sub>Paste your environment variables into Railway and deploy.</sub>
</p>

3. Railway auto-detects the `Dockerfile` and builds on push.
4. Set the Linear webhook URL to `https://<your-app>.up.railway.app/webhook`.
5. Set the Lark event callback URL to `https://<your-app>.up.railway.app/lark/event`.

## Manual Docker Build

```bash
docker build -t larkstack .
docker run -p 3000:3000 \
  -e LINEAR_WEBHOOK_SECRET=your_secret \
  -e LARK_WEBHOOK_URL=https://open.larksuite.com/open-apis/bot/v2/hook/xxx \
  larkstack
```
