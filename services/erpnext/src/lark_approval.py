"""Parse Lark approval form data into ERPNext expense claim fields.

Matches fields by their `name` attribute (label) instead of widget IDs,
so the code works across different approval templates without hardcoding.
"""

import json
import logging

log = logging.getLogger(__name__)

# Lark radio option text → ERPNext Expense Claim Type name
EXPENSE_TYPE_MAP = {
    "差旅费": "Travel",
    "住宿费": "Travel",
    "交通费": "Transportation",
    "招待费": "Food & Beverage",
    "团建费": "Food & Beverage",
    "通讯费": "Communication",
    "办公": "Office Supplies",
    "其他": "Miscellaneous",
}


def parse_expense_form(form_json: str | list) -> dict:
    """Parse Lark approval form into structured expense data.

    Matches widgets by `name` (label) to be resilient to widget ID changes.

    Returns:
        {
            "expense_type": "Travel",
            "type_text": "差旅费",
            "reason": "...",
            "total": 123.45,
            "details": [{"content": "...", "date": "2026-04-01", "amount": 50.0}, ...],
            "attachments": [{"url": "...", "name": "..."}, ...],
        }
    """
    form = json.loads(form_json) if isinstance(form_json, str) else form_json
    widgets = {w.get("name", ""): w for w in form}

    # Expense type — match common label names
    type_text = _extract_radio(widgets, ["报销类型", "费用类型", "类型"])

    # Reason
    reason = _extract_text(widgets, ["报销事由", "事由", "说明", "备注"])

    # Total (formula/amount field)
    total = _extract_amount(widgets, ["费用汇总", "合计", "总金额", "总计"])

    # Detail rows (fieldList)
    details = _extract_detail_rows(widgets, ["费用明细", "明细", "报销明细"])

    # Attachments
    attachments = _extract_attachments(widgets, ["电子发票", "发票", "附件", "发票照片"])

    return {
        "expense_type": EXPENSE_TYPE_MAP.get(type_text, "Miscellaneous"),
        "type_text": type_text,
        "reason": reason,
        "total": total,
        "details": details,
        "attachments": attachments,
    }


def _find_widget(widgets: dict, candidate_names: list[str]) -> dict:
    """Find the first widget matching any of the candidate names."""
    for name in candidate_names:
        if name in widgets:
            return widgets[name]
    return {}


def _extract_radio(widgets: dict, names: list[str]) -> str:
    w = _find_widget(widgets, names)
    value = w.get("value")
    if isinstance(value, list):
        return value[0] if value else ""
    return str(value) if value else ""


def _extract_text(widgets: dict, names: list[str]) -> str:
    return str(_find_widget(widgets, names).get("value", ""))


def _extract_amount(widgets: dict, names: list[str]) -> float:
    raw = _find_widget(widgets, names).get("value", "0")
    try:
        return float(raw)
    except (ValueError, TypeError):
        return 0.0


def _extract_detail_rows(widgets: dict, names: list[str]) -> list[dict]:
    w = _find_widget(widgets, names)
    value = w.get("value")
    if isinstance(value, str):
        try:
            value = json.loads(value)
        except json.JSONDecodeError:
            return []
    if not isinstance(value, list):
        return []

    details = []
    for row in value:
        if not isinstance(row, list):
            continue
        row_by_name = {item.get("name", ""): item for item in row}
        content = _extract_text(row_by_name, ["内容", "摘要", "描述"])
        date = _extract_text(row_by_name, ["日期（年-月-日）", "日期", "消费日期"])
        amount = _extract_amount(row_by_name, ["金额", "费用金额"])
        details.append({"content": content, "date": date, "amount": amount})

    return details


def _extract_attachments(widgets: dict, names: list[str]) -> list[dict]:
    w = _find_widget(widgets, names)
    value = w.get("value")
    if isinstance(value, str):
        try:
            value = json.loads(value)
        except json.JSONDecodeError:
            return []
    if not isinstance(value, list):
        return []
    return [{"url": f["url"], "name": f.get("name", "invoice")} for f in value if isinstance(f, dict) and f.get("url")]
