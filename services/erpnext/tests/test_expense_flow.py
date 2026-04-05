"""End-to-end test for expense claim creation with attachments."""

import asyncio
import os
import sys

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))
from src.erpnext_client import ERPNextClient
from src.lark_approval import parse_expense_form


# Matches actual production form structure (with invoice inside detail row + top-level attachment)
SAMPLE_FORM = [
    {"id": "w1", "name": "报销类型", "type": "radioV2", "value": "差旅费"},
    {"id": "w2", "name": "报销事由", "type": "textarea", "value": "端到端测试"},
    {"id": "w3", "name": "费用汇总", "type": "formula", "value": 42},
    {
        "id": "w4",
        "name": "费用明细",
        "type": "fieldList",
        "value": [
            [
                {"id": "c1", "name": "内容", "type": "input", "value": "测试报销"},
                {"id": "c2", "name": "日期（年-月-日）", "type": "date", "value": "2026-04-05T00:00:00+08:00"},
                {"id": "c3", "name": "金额", "type": "amount", "value": 42},
                {"id": "c4", "name": "发票", "type": "attachmentV2", "ext": "invoice.jpg", "value": ["https://httpbin.org/image/jpeg"]},
            ]
        ],
    },
    {
        "id": "w5",
        "name": "附件",
        "type": "attachmentV2",
        "ext": "receipt.png",
        "value": ["https://httpbin.org/image/png"],
    },
]


async def test_form_parsing():
    print("=== Test: Form Parsing ===")
    expense = parse_expense_form(SAMPLE_FORM)
    assert expense["expense_type"] == "Travel"
    assert expense["total"] == 42.0
    assert len(expense["details"]) == 1
    assert expense["details"][0]["date"] == "2026-04-05"
    assert expense["details"][0]["amount"] == 42.0
    assert len(expense["details"][0]["attachments"]) == 1, f"Expected 1 detail attachment, got {expense['details'][0].get('attachments', [])}"
    assert expense["details"][0]["attachments"][0]["name"] == "invoice.jpg"
    assert len(expense["attachments"]) == 1
    assert expense["attachments"][0]["name"] == "receipt.png"
    print("  Parsing: OK (1 top-level + 1 detail attachment)")


async def test_expense_claim_with_attachments():
    print("\n=== Test: ERPNext Expense Claim + Attachments ===")
    client = ERPNextClient(
        url=os.environ.get("ERPNEXT_URL", "https://erp.acceleai.cn"),
        api_key=os.environ.get("ERPNEXT_API_KEY", "6b0416dec281b8e"),
        api_secret=os.environ.get("ERPNEXT_API_SECRET", "4b42d1346a5a888"),
    )

    expense = parse_expense_form(SAMPLE_FORM)

    # Create
    claim = await client.create_expense_claim(
        employee="HR-EMP-00001",
        expenses=[{
            "expense_type": expense["expense_type"],
            "amount": expense["details"][0]["amount"],
            "date": expense["details"][0]["date"],
            "description": expense["details"][0]["content"],
        }],
        remark="端到端测试",
    )
    name = claim["name"]
    print(f"  Created: {name}")

    # Attach all files (top-level + detail rows)
    all_attachments = list(expense["attachments"])
    for d in expense["details"]:
        all_attachments.extend(d.get("attachments", []))

    for att in all_attachments:
        await client.attach_file(name, att["url"], att["name"])

    # Verify
    resp = await client._client.get(
        "/api/resource/File",
        params={
            "filters": f'[["attached_to_doctype","=","Expense Claim"],["attached_to_name","=","{name}"]]',
            "fields": '["file_name","file_url"]',
        },
    )
    files = resp.json().get("data", [])
    print(f"  Attachments: {len(files)}")
    for f in files:
        print(f"    {f['file_name']} → {f['file_url']}")
    assert len(files) == 2, f"Expected 2 attachments, got {len(files)}"

    # Submit
    await client.submit_expense_claim(name)
    print(f"  Submitted: {name}")

    # Cleanup
    await client._client.put(f"/api/resource/Expense Claim/{name}", json={"docstatus": 2})
    await client._client.delete(f"/api/resource/Expense Claim/{name}")
    print(f"  Cleaned up: {name}")

    await client.close()


async def main():
    await test_form_parsing()
    await test_expense_claim_with_attachments()
    print("\n=== ALL TESTS PASSED ===")


if __name__ == "__main__":
    asyncio.run(main())
