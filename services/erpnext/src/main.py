"""Lark ↔ ERPNext + Keycloak bridge — approval events + employee sync via WebSocket."""

import asyncio
import json
import logging
import threading

import httpx
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
from .invoice_ocr import InvoiceOCRClient
from .keycloak_client import KeycloakClient
from .lark_approval import parse_expense_form
from .sync import full_sync

logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(name)s: %(message)s")
log = logging.getLogger(__name__)

settings = Settings()

erpnext = ERPNextClient(
    url=settings.erpnext.url,
    api_key=settings.erpnext.api_key,
    api_secret=settings.erpnext.api_secret,
)

keycloak = KeycloakClient(
    url=settings.keycloak.url,
    realm=settings.keycloak.realm,
    client_id=settings.keycloak.client_id,
    client_secret=settings.keycloak.client_secret,
)

ocr = InvoiceOCRClient(
    url=settings.invoice_ocr.url,
    token=settings.invoice_ocr.token,
)

_loop: asyncio.AbstractEventLoop | None = None


def _run_async(coro):
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

        # Attach detail-row invoices + OCR
        detail_rows = claim.get("expenses", [])
        ocr_summaries = []
        for i, d in enumerate(expense.get("details", [])):
            row_name = detail_rows[i]["name"] if i < len(detail_rows) else ""
            for att in d.get("attachments", []):
                # Upload file
                await erpnext.upload_and_attach_file(
                    claim["name"], att["url"], att["name"], detail_row_name=row_name,
                )
                # OCR the invoice
                if ocr.enabled and row_name:
                    async with httpx.AsyncClient(timeout=30, follow_redirects=True) as dl:
                        dl_resp = await dl.get(att["url"])
                        if dl_resp.is_success:
                            result = await ocr.recognize(dl_resp.content, att["name"])
                            if result:
                                fields = result.get("fields", {})
                                ocr_data = {
                                    "ocr_invoice_code": fields.get("invoice_code", {}).get("value", ""),
                                    "ocr_invoice_date": fields.get("invoice_date", {}).get("value", ""),
                                    "ocr_seller": fields.get("seller_name", {}).get("value", ""),
                                    "ocr_amount": InvoiceOCRClient.extract_amount(result) or 0,
                                    "ocr_confidence": round(result.get("overall_confidence", 0) * 100, 1),
                                }
                                await erpnext._client.put(
                                    f"/api/resource/Expense Claim Detail/{row_name}",
                                    json=ocr_data,
                                )
                                log.info("OCR data written to row %s", row_name)
                                ocr_summaries.append(InvoiceOCRClient.format_comment(result))

        # Attach top-level attachments
        for att in expense["attachments"]:
            await erpnext.upload_and_attach_file(claim["name"], att["url"], att["name"])

        # Write OCR validation summary to Expense Claim
        if ocr_summaries:
            await erpnext._client.put(
                f"/api/resource/Expense Claim/{claim['name']}",
                json={"ocr_validation": "\n---\n".join(ocr_summaries)},
            )

        await erpnext.submit_expense_claim(claim["name"])
        log.info("Expense Claim %s submitted for instance %s", claim["name"], instance_code)
    except Exception:
        log.exception("Failed to process expense instance %s", instance_code)


async def process_pending_expense(instance_code: str):
    """On PENDING: download invoice attachments, run OCR, add comment to approval."""
    if not ocr.enabled:
        return
    try:
        detail = await _get_instance_detail(instance_code)
        if not detail:
            return

        form_data = detail.get("form", "[]")
        expense = parse_expense_form(form_data)

        # Collect all attachment URLs
        all_attachments = list(expense.get("attachments", []))
        for d in expense.get("details", []):
            all_attachments.extend(d.get("attachments", []))

        if not all_attachments:
            log.info("No attachments to OCR for instance %s", instance_code)
            return

        comments = []
        for att in all_attachments:
            # Download file
            async with httpx.AsyncClient(timeout=30, follow_redirects=True) as dl:
                dl_resp = await dl.get(att["url"])
                if not dl_resp.is_success:
                    continue
                file_content = dl_resp.content

            # Run OCR
            result = await ocr.recognize(file_content, att["name"])
            if result:
                comments.append(InvoiceOCRClient.format_comment(result))

                # Verify amount matches
                ocr_amount = InvoiceOCRClient.extract_amount(result)
                claimed = expense.get("total", 0)
                if ocr_amount and claimed and abs(ocr_amount - claimed) > 0.01:
                    comments.append(
                        f"⚠️ **金额不一致**: 发票 ¥{ocr_amount:.2f} vs 申报 ¥{claimed:.2f}"
                    )

        if comments:
            from lark_oapi.api.approval.v4 import CreateInstanceCommentRequest, InstanceComment
            client = _get_lark_client()
            comment_text = "\n\n---\n\n".join(comments)
            body = InstanceComment.builder().comment(comment_text).build()
            req = CreateInstanceCommentRequest.builder() \
                .instance_id(instance_code) \
                .request_body(body) \
                .build()
            resp = client.approval.v4.instance_comment.create(req)
            if resp.success():
                log.info("Added OCR comment to instance %s", instance_code)
            else:
                log.warning("Failed to add comment: %s %s", resp.code, resp.msg)

    except Exception:
        log.exception("Failed to OCR instance %s", instance_code)


def handle_approval_instance(event: CustomizedEvent):
    data = event.event
    status = data.get("status", "")
    approval_code = data.get("approval_code", "")
    instance_code = data.get("instance_code", "")

    log.info("Approval event: code=%s instance=%s status=%s", approval_code, instance_code, status)

    if settings.expense_approval_code and approval_code != settings.expense_approval_code:
        return

    if status == "PENDING":
        _run_async(process_pending_expense(instance_code))
    elif status == "APPROVED":
        _run_async(process_approved_expense(instance_code))


# ── Contact event handlers (real-time sync to ERPNext + Keycloak) ──


async def _sync_user_by_open_id(open_id: str):
    from lark_oapi.api.contact.v3 import GetUserRequest

    client = _get_lark_client()
    req = GetUserRequest.builder().user_id(open_id).user_id_type("open_id").build()
    resp = client.contact.v3.user.get(req)
    if not resp.success() or not resp.data or not resp.data.user:
        log.warning("Failed to fetch user %s", open_id)
        return

    u = resp.data.user
    is_active = bool(u.status and u.status.is_activated)

    # ERPNext
    existing = await erpnext.find_employee_by_lark_open_id(open_id)
    if existing:
        await erpnext.update_employee_if_changed(
            employee_id=existing, name=u.name, email=u.email or "",
            job_title=u.job_title or "", department="",
            status="Active" if is_active else "Left",
        )
    else:
        await erpnext.create_employee(
            name=u.name, email=u.email or "", lark_open_id=open_id,
            company=settings.company, job_title=u.job_title or "",
        )

    # Keycloak
    if u.email:
        username = u.email.split("@")[0]
        kc_attrs: dict[str, list[str]] = {"lark_open_id": [open_id], "full_name": [u.name]}
        if u.employee_no:
            kc_attrs["employee_id"] = [u.employee_no]

        kc_uid = await keycloak.find_user_by_lark_open_id(open_id)
        if not kc_uid:
            kc_uid = await keycloak.find_user_by_email(u.email)
        if kc_uid:
            await keycloak.update_user(
                kc_uid, firstName=u.name, email=u.email,
                enabled=is_active, attributes=kc_attrs,
            )
        else:
            if "employee_id" not in kc_attrs:
                kc_attrs["employee_id"] = [open_id[-8:]]
            await keycloak.create_user(
                username=username, email=u.email, first_name=u.name,
                attributes=kc_attrs, enabled=is_active,
            )

    log.info("Synced user %s (%s) to ERPNext + Keycloak", u.name, open_id)


async def _deactivate_user(open_id: str):
    # ERPNext
    emp = await erpnext.find_employee_by_lark_open_id(open_id)
    if emp:
        await erpnext.update_employee_if_changed(emp, "", "", "", "", "Left")
        log.info("Deactivated ERPNext employee %s", emp)

    # Keycloak
    kc_uid = await keycloak.find_user_by_lark_open_id(open_id)
    if kc_uid:
        await keycloak.disable_user(kc_uid)
        log.info("Disabled Keycloak user %s", kc_uid)


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
    if open_id:
        log.info("User deleted: %s", open_id)
        _run_async(_deactivate_user(open_id))


def handle_department_created(data: P2ContactDepartmentCreatedV3):
    dept = data.event.object if data.event else None
    if dept:
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
                erpnext, keycloak, settings.company,
            )
        except Exception:
            log.exception("Periodic sync failed")


# ── Main ──────────────────────────────────────────────────


def main():
    global _loop
    _loop = asyncio.new_event_loop()

    log.info("Starting Lark ↔ ERPNext + Keycloak bridge")
    log.info("ERPNext: %s | Keycloak: %s/%s", settings.erpnext.url, settings.keycloak.url, settings.keycloak.realm)
    log.info("Company: %s | Sync interval: %dh", settings.company, settings.sync_interval_hours)

    # Initial full sync
    _loop.run_until_complete(
        full_sync(
            settings.lark.app_id, settings.lark.app_secret,
            erpnext, keycloak, settings.company,
        )
    )

    # Periodic sync in background
    threading.Thread(target=lambda: _loop.run_forever(), daemon=True).start()
    asyncio.run_coroutine_threadsafe(periodic_sync(), _loop)

    # WebSocket event handlers
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
