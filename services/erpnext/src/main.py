"""Lark ↔ ERPNext bridge — approval events + employee sync via WebSocket."""

import asyncio
import json
import logging
import threading

import lark_oapi as lark
from lark_oapi.api.contact.v3 import (
    P2ContactUserCreatedV3,
    P2ContactUserUpdatedV3,
    P2ContactUserDeletedV3,
    P2ContactDepartmentCreatedV3,
    P2ContactDepartmentUpdatedV3,
    P2ContactDepartmentDeletedV3,
)
from lark_oapi.event.custom import CustomizedEvent
from lark_oapi.event.dispatcher_handler import EventDispatcherHandler
from lark_oapi.ws import Client as WSClient

from .config import Settings
from .erpnext_client import ERPNextClient
from .lark_approval import parse_expense_form
from .sync import full_sync, fetch_all_departments

logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(name)s: %(message)s")
log = logging.getLogger(__name__)

settings = Settings()
erpnext = ERPNextClient(
    url=settings.erpnext.url,
    api_key=settings.erpnext.api_key,
    api_secret=settings.erpnext.api_secret,
)

_loop: asyncio.AbstractEventLoop | None = None


def _run_async(coro):
    """Schedule a coroutine on the shared event loop."""
    if _loop:
        asyncio.run_coroutine_threadsafe(coro, _loop)
    else:
        asyncio.run(coro)


def _get_lark_client() -> lark.Client:
    return lark.Client.builder().app_id(settings.lark.app_id).app_secret(settings.lark.app_secret).build()


# ── Approval event handler ────────────────────────────────


async def _get_instance_detail(instance_code: str) -> dict:
    from lark_oapi.api.approval.v4 import GetInstanceRequest

    client = _get_lark_client()
    req = GetInstanceRequest.builder().instance_id(instance_code).build()
    resp = client.approval.v4.instance.get(req)
    if not resp.success():
        log.error("Failed to get instance %s: %s", instance_code, resp.msg)
        return {}
    return json.loads(lark.JSON.marshal(resp.data))


async def _get_user_email(open_id: str) -> str | None:
    from lark_oapi.api.contact.v3 import GetUserRequest

    client = _get_lark_client()
    req = GetUserRequest.builder().user_id(open_id).user_id_type("open_id").build()
    resp = client.contact.v3.user.get(req)
    if resp.success() and resp.data and resp.data.user:
        return resp.data.user.email
    return None


async def process_approved_expense(instance_code: str):
    """Fetch approved expense from Lark, create Expense Claim in ERPNext."""
    try:
        detail = await _get_instance_detail(instance_code)
        if not detail:
            return

        form_data = detail.get("form", "[]")
        expense = parse_expense_form(form_data)
        log.info(
            "Expense: type=%s reason=%s total=%s details=%d",
            expense["type_text"], expense["reason"][:30], expense["total"], len(expense["details"]),
        )

        if expense["total"] <= 0 and not expense["details"]:
            log.warning("Skipping zero-amount expense for instance %s", instance_code)
            return

        # Resolve employee by open_id, fallback to email
        open_id = detail.get("open_id", "") or detail.get("user_id", "")
        employee_id = await erpnext.find_employee_by_lark_open_id(open_id) if open_id else None
        if not employee_id and open_id:
            email = await _get_user_email(open_id)
            if email:
                employee_id = await erpnext.find_employee_by_email(email)

        if not employee_id:
            log.error("No ERPNext employee for Lark user %s (instance %s)", open_id, instance_code)
            return

        erpnext_type = expense["expense_type"]
        rows = (
            [
                {"expense_type": erpnext_type, "amount": d["amount"], "date": d["date"], "description": d["content"]}
                for d in expense["details"]
                if d["amount"] > 0
            ]
            if expense["details"]
            else [{"expense_type": erpnext_type, "amount": expense["total"]}]
        )

        claim = await erpnext.create_expense_claim(
            employee=employee_id,
            expenses=rows,
            remark=f"{expense['type_text']} - {expense['reason']}\n飞书审批: {instance_code}",
        )

        for att in expense["attachments"]:
            await erpnext.attach_file(claim["name"], att["url"], att["name"])

        await erpnext.submit_expense_claim(claim["name"])
        log.info("Expense Claim %s submitted for instance %s", claim["name"], instance_code)
    except Exception:
        log.exception("Failed to process expense instance %s", instance_code)


def handle_approval_instance(event: CustomizedEvent):
    data = event.event
    status = data.get("status", "")
    approval_code = data.get("approval_code", "")
    instance_code = data.get("instance_code", "")

    log.info("Approval event: code=%s instance=%s status=%s", approval_code, instance_code, status)

    if status != "APPROVED":
        return
    if settings.expense_approval_code and approval_code != settings.expense_approval_code:
        return

    _run_async(process_approved_expense(instance_code))


# ── Contact event handlers (real-time employee sync) ──────


async def _sync_user_by_open_id(open_id: str):
    """Fetch a single user from Lark and upsert into ERPNext."""
    from lark_oapi.api.contact.v3 import GetUserRequest

    client = _get_lark_client()
    req = GetUserRequest.builder().user_id(open_id).user_id_type("open_id").build()
    resp = client.contact.v3.user.get(req)
    if not resp.success() or not resp.data or not resp.data.user:
        log.warning("Failed to fetch user %s", open_id)
        return

    u = resp.data.user
    existing = await erpnext.find_employee_by_lark_open_id(open_id)

    if existing:
        await erpnext.update_employee_if_changed(
            employee_id=existing,
            name=u.name,
            email=u.email or "",
            job_title=u.job_title or "",
            department="",
            status="Active" if (u.status and u.status.is_activated) else "Left",
        )
        log.info("Updated employee %s (%s)", existing, u.name)
    else:
        await erpnext.create_employee(
            name=u.name,
            email=u.email or "",
            lark_open_id=open_id,
            company=settings.company,
            job_title=u.job_title or "",
        )
        log.info("Created employee for %s", u.name)


def handle_user_created(data: P2ContactUserCreatedV3):
    open_id = data.event.object.open_id if data.event and data.event.object else None
    if open_id:
        log.info("User created: %s", open_id)
        _run_async(_sync_user_by_open_id(open_id))


def handle_user_updated(data: P2ContactUserUpdatedV3):
    open_id = data.event.object.open_id if data.event and data.event.object else None
    if open_id:
        log.info("User updated: %s", open_id)
        _run_async(_sync_user_by_open_id(open_id))


def handle_user_deleted(data: P2ContactUserDeletedV3):
    open_id = data.event.object.open_id if data.event and data.event.object else None
    if not open_id:
        return
    log.info("User deleted: %s", open_id)

    async def _deactivate():
        emp = await erpnext.find_employee_by_lark_open_id(open_id)
        if emp:
            await erpnext.update_employee_if_changed(emp, "", "", "", "", "Left")
            log.info("Deactivated employee %s", emp)

    _run_async(_deactivate())


def handle_department_created(data: P2ContactDepartmentCreatedV3):
    dept = data.event.object if data.event else None
    if not dept:
        return
    log.info("Department created: %s", dept.name)
    _run_async(erpnext.create_department(dept.name, settings.company))


def handle_department_updated(data: P2ContactDepartmentUpdatedV3):
    log.info("Department updated event received")


def handle_department_deleted(data: P2ContactDepartmentDeletedV3):
    log.info("Department deleted event received")


# ── Periodic full sync ────────────────────────────────────


async def periodic_sync():
    interval = settings.sync_interval_hours * 3600
    while True:
        await asyncio.sleep(interval)
        try:
            await full_sync(
                settings.lark.app_id, settings.lark.app_secret,
                erpnext, settings.company, settings.company_abbr,
            )
        except Exception:
            log.exception("Periodic sync failed")


# ── Main entry point ──────────────────────────────────────


def main():
    global _loop
    _loop = asyncio.new_event_loop()

    log.info("Starting Lark ↔ ERPNext bridge")
    log.info("ERPNext: %s | Company: %s", settings.erpnext.url, settings.company)
    log.info("Expense approval code: %s", settings.expense_approval_code or "(all)")
    log.info("Sync interval: %dh", settings.sync_interval_hours)

    # Initial full sync
    _loop.run_until_complete(
        full_sync(
            settings.lark.app_id, settings.lark.app_secret,
            erpnext, settings.company, settings.company_abbr,
        )
    )

    # Start periodic sync in background
    threading.Thread(target=lambda: _loop.run_forever(), daemon=True).start()
    asyncio.run_coroutine_threadsafe(periodic_sync(), _loop)

    # Register event handlers
    event_handler = (
        EventDispatcherHandler.builder("", "")
        .register_p1_customized_event("approval_instance", handle_approval_instance)
        .register_p2_contact_user_created_v3(handle_user_created)
        .register_p2_contact_user_updated_v3(handle_user_updated)
        .register_p2_contact_user_deleted_v3(handle_user_deleted)
        .register_p2_contact_department_created_v3(handle_department_created)
        .register_p2_contact_department_updated_v3(handle_department_updated)
        .register_p2_contact_department_deleted_v3(handle_department_deleted)
        .build()
    )

    ws_client = WSClient(
        app_id=settings.lark.app_id,
        app_secret=settings.lark.app_secret,
        event_handler=event_handler,
        log_level=lark.LogLevel.INFO,
        auto_reconnect=True,
    )

    log.info("Connecting to Lark WebSocket...")
    ws_client.start()


if __name__ == "__main__":
    main()
