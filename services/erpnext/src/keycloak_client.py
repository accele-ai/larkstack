"""Keycloak Admin REST API client."""

import logging
import time

import httpx

log = logging.getLogger(__name__)


class KeycloakClient:
    def __init__(self, url: str, realm: str, client_id: str, client_secret: str):
        self.base_url = url.rstrip("/")
        self.realm = realm
        self._client_id = client_id
        self._client_secret = client_secret
        self._token: str = ""
        self._token_expires_at: float = 0
        self._client = httpx.AsyncClient(timeout=30)

    async def _ensure_token(self):
        """Obtain or refresh admin token via client credentials grant."""
        if self._token and time.time() < self._token_expires_at - 30:
            return

        resp = await self._client.post(
            f"{self.base_url}/realms/{self.realm}/protocol/openid-connect/token",
            data={
                "grant_type": "client_credentials",
                "client_id": self._client_id,
                "client_secret": self._client_secret,
            },
            headers={"Content-Type": "application/x-www-form-urlencoded"},
        )
        resp.raise_for_status()
        data = resp.json()
        self._token = data["access_token"]
        self._token_expires_at = time.time() + data.get("expires_in", 300)
        log.debug("Keycloak token refreshed, expires in %ds", data.get("expires_in", 300))

    async def _request(self, method: str, path: str, **kwargs) -> httpx.Response:
        await self._ensure_token()
        url = f"{self.base_url}/admin/realms/{self.realm}{path}"
        headers = {"Authorization": f"Bearer {self._token}", "Content-Type": "application/json"}
        return await self._client.request(method, url, headers=headers, **kwargs)

    # ── User lookup ────────────────────────────────────────

    async def find_user_by_email(self, email: str) -> str | None:
        resp = await self._request("GET", "/users", params={"email": email, "exact": "true"})
        if resp.is_success:
            users = resp.json()
            return users[0]["id"] if users else None
        return None

    async def find_user_by_attribute(self, attr: str, value: str) -> str | None:
        resp = await self._request("GET", "/users", params={"q": f"{attr}:{value}"})
        if resp.is_success:
            for u in resp.json():
                attrs = u.get("attributes", {})
                if value in attrs.get(attr, []):
                    return u["id"]
        return None

    async def find_user_by_lark_open_id(self, lark_open_id: str) -> str | None:
        return await self.find_user_by_attribute("lark_open_id", lark_open_id)

    # ── User management ───────────────────────────────────

    async def create_user(
        self,
        username: str,
        email: str,
        first_name: str,
        *,
        attributes: dict[str, list[str]] | None = None,
        enabled: bool = True,
    ) -> str | None:
        payload: dict = {
            "username": username,
            "email": email,
            "firstName": first_name,
            "enabled": enabled,
            "emailVerified": True,
        }
        if attributes:
            payload["attributes"] = attributes

        resp = await self._request("POST", "/users", json=payload)
        if resp.status_code == 201:
            location = resp.headers.get("Location", "")
            user_id = location.rsplit("/", 1)[-1] if location else None
            log.info("Created Keycloak user %s (%s)", username, user_id)
            return user_id
        elif resp.status_code == 409:
            log.debug("Keycloak user %s already exists", username)
            return await self.find_user_by_email(email)
        else:
            log.warning("Failed to create Keycloak user %s: %s %s", username, resp.status_code, resp.text[:200])
            return None

    async def update_user(self, user_id: str, **fields) -> bool:
        existing = await self.get_user(user_id)
        if not existing:
            return False

        # Merge: keep existing fields, overlay with new values
        payload = {
            "username": existing["username"],
            "email": fields.get("email", existing.get("email", "")),
            "firstName": fields.get("firstName", existing.get("firstName", "")),
            "lastName": fields.get("lastName", existing.get("lastName", "")),
            "enabled": fields.get("enabled", existing.get("enabled", True)),
            "attributes": {**existing.get("attributes", {}), **fields.get("attributes", {})},
        }

        resp = await self._request("PUT", f"/users/{user_id}", json=payload)
        if resp.is_success:
            log.info("Updated Keycloak user %s (%s)", existing["username"], user_id)
            return True
        log.warning("Failed to update Keycloak user %s: %s %s", user_id, resp.status_code, resp.text[:200])
        return False

    async def disable_user(self, user_id: str) -> bool:
        return await self.update_user(user_id, enabled=False)

    async def get_user(self, user_id: str) -> dict | None:
        resp = await self._request("GET", f"/users/{user_id}")
        return resp.json() if resp.is_success else None

    async def close(self):
        await self._client.aclose()
