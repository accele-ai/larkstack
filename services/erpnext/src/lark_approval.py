"""Parse Lark approval form data into ERPNext expense claim fields.

Matches fields by their `name` attribute (label) instead of widget IDs,
so the code works across different approval templates without hardcoding.
"""

import json
import logging
from datetime import date

from dateutil.parser import parse as parse_date

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

    type_text = _extract_radio(widgets, ["报销类型", "费用类型", "类型"])
    reason = _extract_text(widgets, ["报销事由", "事由", "说明", "备注"])
    total = _extract_amount(widgets, ["费用汇总", "合计", "总金额", "总计"])
    details = _extract_detail_rows(widgets, ["费用明细", "明细", "报销明细"])
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
    for name in candidate_names:
        if name in widgets:
            return widgets[name]
    return {}


def _extract_radio(widgets: dict, names: list[str]) -> str:
    w = _find_widget(widgets, names)
    value = w.get("value")
    if value is None:
        return ""
    if isinstance(value, dict):
        return value.get("text", value.get("value", ""))
    if isinstance(value, list):
        first = value[0] if value else ""
        return first.get("text", str(first)) if isinstance(first, dict) else str(first)
    return str(value)


def _extract_text(widgets: dict, names: list[str]) -> str:
    value = _find_widget(widgets, names).get("value")
    if value is None:
        return ""
    return str(value)


def _to_date_str(raw: str) -> str:
    """Parse any date format into YYYY-MM-DD string."""
    if not raw:
        return str(date.today())
    try:
        return parse_date(raw).strftime("%Y-%m-%d")
    except (ValueError, TypeError):
        # Fallback: try to extract YYYY-MM-DD prefix
        if len(raw) >= 10 and raw[4] == "-" and raw[7] == "-":
            return raw[:10]
        return str(date.today())


def _extract_amount(widgets: dict, names: list[str]) -> float:
    raw = _find_widget(widgets, names).get("value")
    if raw is None:
        return 0.0
    if isinstance(raw, (int, float)):
        return float(raw)
    try:
        return float(str(raw).replace(",", ""))
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
        row_by_name = {item.get("name", ""): item for item in row if isinstance(item, dict)}
        content = _extract_text(row_by_name, ["内容", "摘要", "描述"])
        raw_date = _extract_text(row_by_name, ["日期（年-月-日）", "日期", "消费日期"])
        amount = _extract_amount(row_by_name, ["金额", "费用金额"])
        details.append({
            "content": content,
            "date": _to_date_str(raw_date),
            "amount": amount,
        })

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

    ext = w.get("ext", "")
    result = []
    for i, f in enumerate(value):
        if isinstance(f, str) and f.startswith("http"):
            fname = ext if isinstance(ext, str) and ext else f"attachment_{i}"
            result.append({"url": f, "name": fname})
        elif isinstance(f, dict) and f.get("url"):
            result.append({"url": f["url"], "name": f.get("name", f"attachment_{i}")})
    return result
