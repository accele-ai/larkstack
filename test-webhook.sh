#!/bin/bash
# Usage: ./test-webhook.sh <your-linear-webhook-secret>

ENDPOINT="https://linear-lark-bridge-production.up.railway.app/webhook"
SECRET="${1:?Usage: ./test-webhook.sh <your-LINEAR_WEBHOOK_SECRET>}"

echo "=== 1. Health check ==="
curl -s https://linear-lark-bridge-production.up.railway.app/health
echo -e "\n"

echo "=== 2. Missing signature → expect 401 ==="
curl -s -o /dev/null -w "HTTP Status: %{http_code}\n" \
  -X POST "$ENDPOINT" \
  -H "Content-Type: application/json" \
  -d '{}'
echo ""

echo "=== 3. Wrong signature → expect 401 ==="
curl -s -o /dev/null -w "HTTP Status: %{http_code}\n" \
  -X POST "$ENDPOINT" \
  -H "Content-Type: application/json" \
  -H "linear-signature: deadbeef" \
  -d '{}'
echo ""

echo "=== 4. Ignored event type → expect 200, no Lark message ==="
PAYLOAD_IGNORE='{"action":"delete","type":"Issue","url":"https://linear.app/test","data":{"id":"fake-001","title":"Ignored","priority":0,"identifier":"TEST-0","state":{"name":"Triage"},"assignee":null}}'
SIG_IGNORE=$(printf '%s' "$PAYLOAD_IGNORE" | openssl dgst -sha256 -hmac "$SECRET" | awk '{print $2}')
curl -s -o /dev/null -w "HTTP Status: %{http_code}\n" \
  -X POST "$ENDPOINT" \
  -H "Content-Type: application/json" \
  -H "linear-signature: $SIG_IGNORE" \
  -d "$PAYLOAD_IGNORE"
echo ""

echo "=== 5. Issue create (Urgent, with description) → expect 200 + Lark card ==="
PAYLOAD_CREATE='{"action":"create","type":"Issue","url":"https://linear.app/team/issue/TEST-1/auth-500","data":{"id":"fake-002","title":"Auth service returns 500 on login","priority":1,"identifier":"TEST-1","state":{"name":"In Progress"},"assignee":{"name":"QA Bot","email":"qa@example.com"},"description":"Users are seeing 500 errors when attempting to log in via the /auth/login endpoint. This started after the latest deploy and affects approximately 30% of login attempts. Stack trace points to a null pointer in the session handler."}}'
SIG_CREATE=$(printf '%s' "$PAYLOAD_CREATE" | openssl dgst -sha256 -hmac "$SECRET" | awk '{print $2}')
curl -s -o /dev/null -w "HTTP Status: %{http_code}\n" \
  -X POST "$ENDPOINT" \
  -H "Content-Type: application/json" \
  -H "linear-signature: $SIG_CREATE" \
  -d "$PAYLOAD_CREATE"
echo ""

echo "=== 6. Issue update with status change (Medium, unassigned) → expect 200 + Lark card with changes ==="
PAYLOAD_UPDATE='{"action":"update","type":"Issue","url":"https://linear.app/team/issue/TEST-2/update-dashboard","updatedFrom":{"state":{"name":"Todo"},"priority":4},"data":{"id":"fake-003","title":"Update dashboard layout","priority":3,"identifier":"TEST-2","state":{"name":"In Progress"},"assignee":null}}'
SIG_UPDATE=$(printf '%s' "$PAYLOAD_UPDATE" | openssl dgst -sha256 -hmac "$SECRET" | awk '{print $2}')
curl -s -o /dev/null -w "HTTP Status: %{http_code}\n" \
  -X POST "$ENDPOINT" \
  -H "Content-Type: application/json" \
  -H "linear-signature: $SIG_UPDATE" \
  -d "$PAYLOAD_UPDATE"
echo ""

echo "=== 7. Issue update with assignee change → expect 200 + Lark card with assignee change ==="
PAYLOAD_ASSIGN='{"action":"update","type":"Issue","url":"https://linear.app/team/issue/TEST-3/fix-bug","updatedFrom":{"assigneeId":"old-user-id","assignee":{"name":"Old Developer"}},"data":{"id":"fake-004","title":"Fix critical payment bug","priority":2,"identifier":"TEST-3","state":{"name":"In Progress"},"assignee":{"name":"New Developer","email":"new-dev@example.com"}}}'
SIG_ASSIGN=$(printf '%s' "$PAYLOAD_ASSIGN" | openssl dgst -sha256 -hmac "$SECRET" | awk '{print $2}')
curl -s -o /dev/null -w "HTTP Status: %{http_code}\n" \
  -X POST "$ENDPOINT" \
  -H "Content-Type: application/json" \
  -H "linear-signature: $SIG_ASSIGN" \
  -d "$PAYLOAD_ASSIGN"
echo ""

echo "=== 8. Comment create → expect 200 + Lark card ==="
PAYLOAD_COMMENT='{"action":"create","type":"Comment","url":"https://linear.app/team/issue/TEST-1/auth-500#comment-abc","data":{"id":"comment-001","body":"I investigated this and the root cause is a missing null check in SessionHandler.java line 142. The session object can be null when the Redis connection times out. Working on a fix now.","issue":{"identifier":"TEST-1","title":"Auth service returns 500 on login"},"user":{"name":"Senior Dev","email":"senior@example.com"}}}'
SIG_COMMENT=$(printf '%s' "$PAYLOAD_COMMENT" | openssl dgst -sha256 -hmac "$SECRET" | awk '{print $2}')
curl -s -o /dev/null -w "HTTP Status: %{http_code}\n" \
  -X POST "$ENDPOINT" \
  -H "Content-Type: application/json" \
  -H "linear-signature: $SIG_COMMENT" \
  -d "$PAYLOAD_COMMENT"
echo ""

echo "=== 9. Lark challenge verification → expect 200 + challenge echo ==="
LARK_EVENT_ENDPOINT="https://linear-lark-bridge-production.up.railway.app/lark/event"
curl -s -w "\nHTTP Status: %{http_code}\n" \
  -X POST "$LARK_EVENT_ENDPOINT" \
  -H "Content-Type: application/json" \
  -d '{"type":"url_verification","challenge":"test-challenge-token-123"}'
echo ""

echo "=== Done. Check Railway logs and your Lark group. ==="
