"""ERPNext REST API client."""

import logging

import httpx

log = logging.getLogger(__name__)


class ERPNextClient:
    def __init__(self, url: str, api_key: str, api_secret: str):
        self.base_url = url.rstrip("/")
        self._client = httpx.AsyncClient(
            base_url=self.base_url,
            headers={
                "Authorization": f"token {api_key}:{api_secret}",
                "Content-Type": "application/json",
            },
            timeout=30,
        )

    # ── Expense Claim ──────────────────────────────────────

    async def create_expense_claim(
        self,
        employee: str,
        expenses: list[dict],
        *,
        remark: str = "",
    ) -> dict:
        from datetime import date as dt_date

        payload = {
            "doctype": "Expense Claim",
            "employee": employee,
            "expense_type": expenses[0]["expense_type"] if expenses else "Miscellaneous",
            "exchange_rate": 1,
            "expenses": [
                {
                    "expense_type": e["expense_type"],
                    "amount": e["amount"],
                    "expense_date": e.get("date") or str(dt_date.today()),
                    "description": e.get("description", ""),
                }
                for e in expenses
            ],
        }
        if remark:
            payload["remark"] = remark

        resp = await self._client.post("/api/resource/Expense Claim", json=payload)
        if not resp.is_success:
            log.error("Expense Claim creation failed: %s %s", resp.status_code, resp.text[:300])
        resp.raise_for_status()
        result = resp.json()
        claim_name = result["data"]["name"]
        log.info("Created Expense Claim %s for %s", claim_name, employee)
        return result["data"]

    async def submit_expense_claim(self, name: str) -> dict:
        resp = await self._client.put(
            f"/api/resource/Expense Claim/{name}",
            json={"docstatus": 1, "approval_status": "Approved"},
        )
        resp.raise_for_status()
        log.info("Submitted Expense Claim %s", name)
        return resp.json()["data"]

    async def attach_file(self, docname: str, file_url: str, filename: str) -> None:
        """Download file from URL and upload to ERPNext as attachment."""
        try:
            # Download file content from Lark
            async with httpx.AsyncClient(timeout=30, follow_redirects=True) as dl:
                dl_resp = await dl.get(file_url)
                if not dl_resp.is_success:
                    log.warning("Failed to download %s: %s", filename, dl_resp.status_code)
                    return
                file_content = dl_resp.content
                content_type = dl_resp.headers.get("content-type", "application/octet-stream")

            # Upload to ERPNext
            resp = await self._client.post(
                "/api/method/upload_file",
                data={
                    "doctype": "Expense Claim",
                    "docname": docname,
                    "is_private": "1",
                },
                files={"file": (filename, file_content, content_type)},
                headers={"Content-Type": None},  # let httpx set multipart boundary
            )
            if resp.is_success:
                log.info("Attached %s to %s", filename, docname)
            else:
                log.warning("Failed to upload %s to %s: %s", filename, docname, resp.text[:200])
        except Exception as e:
            log.warning("Failed to attach %s to %s: %s", filename, docname, e)

    # ── Employee ───────────────────────────────────────────

    async def find_employee_by_lark_open_id(self, lark_open_id: str) -> str | None:
        resp = await self._client.get(
            "/api/resource/Employee",
            params={
                "filters": f'[["lark_open_id","=","{lark_open_id}"]]',
                "fields": '["name"]',
                "limit_page_length": 1,
            },
        )
        if resp.is_success:
            data = resp.json().get("data", [])
            return data[0]["name"] if data else None
        return None

    async def find_employee_by_email(self, email: str) -> str | None:
        resp = await self._client.get(
            "/api/resource/Employee",
            params={
                "filters": f'[["company_email","=","{email}"],["status","=","Active"]]',
                "fields": '["name"]',
                "limit_page_length": 1,
            },
        )
        if resp.is_success:
            data = resp.json().get("data", [])
            return data[0]["name"] if data else None
        return None

    async def create_employee(
        self,
        name: str,
        email: str,
        lark_open_id: str,
        company: str,
        department: str = "",
        job_title: str = "",
    ) -> str:
        from datetime import date

        payload: dict = {
            "doctype": "Employee",
            "employee_name": name,
            "first_name": name,
            "company": company,
            "lark_open_id": lark_open_id,
            "status": "Active",
            "gender": "Male",
            "date_of_birth": "2000-01-01",
            "date_of_joining": str(date.today()),
        }
        if email:
            payload["company_email"] = email
        if department:
            payload["department"] = department
        if job_title:
            payload["designation"] = job_title

        resp = await self._client.post("/api/resource/Employee", json=payload)
        resp.raise_for_status()
        emp_id = resp.json()["data"]["name"]
        log.info("Created employee %s (%s)", emp_id, name)
        return emp_id

    async def update_employee_if_changed(
        self,
        employee_id: str,
        name: str,
        email: str,
        job_title: str,
        department: str,
        status: str,
    ) -> bool:
        resp = await self._client.get(
            f"/api/resource/Employee/{employee_id}",
            params={"fields": '["employee_name","company_email","designation","department","status"]'},
        )
        if not resp.is_success:
            return False

        current = resp.json()["data"]
        updates: dict = {}

        if current.get("employee_name") != name:
            updates["employee_name"] = name
            updates["first_name"] = name
        if email and current.get("company_email") != email:
            updates["company_email"] = email
        if job_title and current.get("designation") != job_title:
            updates["designation"] = job_title
        if department and current.get("department") != department:
            updates["department"] = department
        if current.get("status") != status:
            updates["status"] = status

        if not updates:
            return False

        resp = await self._client.put(f"/api/resource/Employee/{employee_id}", json=updates)
        if resp.is_success:
            log.info("Updated employee %s: %s", employee_id, list(updates.keys()))
        return resp.is_success

    # ── Department ─────────────────────────────────────────

    async def find_department_by_name(self, name: str, company: str) -> str | None:
        resp = await self._client.get(
            "/api/resource/Department",
            params={
                "filters": f'[["department_name","=","{name}"],["company","=","{company}"]]',
                "fields": '["name"]',
                "limit_page_length": 1,
            },
        )
        if resp.is_success:
            data = resp.json().get("data", [])
            return data[0]["name"] if data else None
        return None

    async def get_root_department(self) -> str:
        """Find the root department name (locale-dependent)."""
        resp = await self._client.get(
            "/api/resource/Department",
            params={
                "filters": '[["is_group","=",1]]',
                "fields": '["name"]',
                "limit_page_length": 1,
                "order_by": "lft asc",
            },
        )
        if resp.is_success:
            data = resp.json().get("data", [])
            if data:
                return data[0]["name"]
        return "All Departments"

    async def create_department(self, name: str, company: str) -> str:
        root = await self.get_root_department()
        resp = await self._client.post(
            "/api/resource/Department",
            json={
                "doctype": "Department",
                "department_name": name,
                "company": company,
                "parent_department": root,
            },
        )
        resp.raise_for_status()
        dept_id = resp.json()["data"]["name"]
        log.info("Created department %s", dept_id)
        return dept_id

    async def close(self):
        await self._client.aclose()
