"""Lark → ERPNext employee & department sync."""

import json
import logging
from dataclasses import dataclass

import lark_oapi as lark
from lark_oapi.api.contact.v3 import (
    ListDepartmentRequest,
    ListUserRequest,
)

from .erpnext_client import ERPNextClient

log = logging.getLogger(__name__)


@dataclass
class LarkUser:
    open_id: str
    name: str
    email: str
    mobile: str
    department_ids: list[str]
    job_title: str
    is_active: bool


@dataclass
class LarkDepartment:
    open_department_id: str
    name: str
    parent_open_department_id: str


def _build_client(app_id: str, app_secret: str) -> lark.Client:
    return lark.Client.builder().app_id(app_id).app_secret(app_secret).build()


def fetch_all_departments(app_id: str, app_secret: str) -> list[LarkDepartment]:
    """Fetch all departments from Lark."""
    client = _build_client(app_id, app_secret)
    departments = []
    page_token = None

    while True:
        builder = (
            ListDepartmentRequest.builder()
            .parent_department_id("0")
            .fetch_child(True)
            .page_size(50)
            .department_id_type("open_department_id")
        )
        if page_token:
            builder = builder.page_token(page_token)

        resp = client.contact.v3.department.list(builder.build())
        if not resp.success():
            log.error("Failed to list departments: %s %s", resp.code, resp.msg)
            break

        for d in resp.data.items or []:
            departments.append(LarkDepartment(
                open_department_id=d.open_department_id,
                name=d.name,
                parent_open_department_id=d.parent_department_id or "",
            ))

        if not resp.data.has_more:
            break
        page_token = resp.data.page_token

    log.info("Fetched %d departments from Lark", len(departments))
    return departments


def fetch_all_users(app_id: str, app_secret: str) -> list[LarkUser]:
    """Fetch all users from Lark."""
    client = _build_client(app_id, app_secret)
    users = []
    page_token = None

    while True:
        builder = (
            ListUserRequest.builder()
            .department_id("0")
            .page_size(50)
            .user_id_type("open_id")
        )
        if page_token:
            builder = builder.page_token(page_token)

        resp = client.contact.v3.user.list(builder.build())
        if not resp.success():
            log.error("Failed to list users: %s %s", resp.code, resp.msg)
            break

        for u in resp.data.items or []:
            users.append(LarkUser(
                open_id=u.open_id,
                name=u.name,
                email=u.email or "",
                mobile=u.mobile or "",
                department_ids=u.department_ids or [],
                job_title=u.job_title or "",
                is_active=bool(u.status and u.status.is_activated),
            ))

        if not resp.data.has_more:
            break
        page_token = resp.data.page_token

    log.info("Fetched %d users from Lark", len(users))
    return users


async def sync_departments(
    erpnext: ERPNextClient,
    departments: list[LarkDepartment],
    company: str,
    company_abbr: str,
):
    """Sync departments to ERPNext."""
    for dept in departments:
        existing = await erpnext.find_department_by_name(dept.name, company)
        if existing:
            log.debug("Department exists: %s", dept.name)
            continue
        await erpnext.create_department(dept.name, company)
        log.info("Created department: %s", dept.name)


async def sync_employees(
    erpnext: ERPNextClient,
    users: list[LarkUser],
    departments: list[LarkDepartment],
    company: str,
):
    """Sync users to ERPNext as employees."""
    dept_map = {d.open_department_id: d.name for d in departments}
    created, updated, skipped = 0, 0, 0

    for user in users:
        dept_name = ""
        for did in user.department_ids:
            if did in dept_map:
                dept_name = dept_map[did]
                break

        existing = await erpnext.find_employee_by_lark_open_id(user.open_id)

        if existing:
            changed = await erpnext.update_employee_if_changed(
                employee_id=existing,
                name=user.name,
                email=user.email,
                job_title=user.job_title,
                department=dept_name,
                status="Active" if user.is_active else "Left",
            )
            if changed:
                updated += 1
            else:
                skipped += 1
        else:
            await erpnext.create_employee(
                name=user.name,
                email=user.email,
                lark_open_id=user.open_id,
                company=company,
                department=dept_name,
                job_title=user.job_title,
            )
            created += 1

    log.info("Employee sync done: created=%d updated=%d unchanged=%d", created, updated, skipped)


async def full_sync(app_id: str, app_secret: str, erpnext: ERPNextClient, company: str, company_abbr: str):
    """Run a full sync: departments then employees."""
    log.info("Starting full sync...")
    departments = fetch_all_departments(app_id, app_secret)
    await sync_departments(erpnext, departments, company, company_abbr)

    users = fetch_all_users(app_id, app_secret)
    await sync_employees(erpnext, users, departments, company)
    log.info("Full sync complete.")
