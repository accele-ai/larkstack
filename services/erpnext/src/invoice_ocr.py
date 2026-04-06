"""Invoice OCR client — validates invoices before/after approval."""

import logging

import httpx

log = logging.getLogger(__name__)


class InvoiceOCRClient:
    def __init__(self, url: str, token: str):
        self.url = url.rstrip("/")
        self.token = token
        self.enabled = bool(url and token)

    async def recognize(self, file_content: bytes, filename: str) -> dict | None:
        """Send file to OCR service and return structured result.

        Returns dict with keys: invoice_type, fields, validation, overall_confidence
        Returns None on failure.
        """
        if not self.enabled:
            return None

        try:
            async with httpx.AsyncClient(timeout=30) as client:
                resp = await client.post(
                    f"{self.url}/api/v1/recognize",
                    headers={"Authorization": f"Bearer {self.token}"},
                    files={"file": (filename, file_content)},
                )

            if not resp.is_success:
                log.warning("OCR failed for %s: %s", filename, resp.status_code)
                return None

            result = resp.json()
            log.info(
                "OCR result for %s: type=%s confidence=%.2f",
                filename, result.get("invoice_type", "unknown"), result.get("overall_confidence", 0),
            )
            return result
        except Exception as e:
            log.warning("OCR error for %s: %s", filename, e)
            return None

    @staticmethod
    def format_comment(ocr_result: dict) -> str:
        """Format OCR result as markdown for Lark approval comment."""
        fields = ocr_result.get("fields", {})
        validations = ocr_result.get("validation", [])
        confidence = ocr_result.get("overall_confidence", 0)

        lines = [f"📄 **发票识别结果** (置信度: {confidence:.0%})\n"]

        field_labels = {
            "invoice_code": "发票代码",
            "invoice_number": "发票号码",
            "invoice_date": "开票日期",
            "buyer_name": "购买方",
            "seller_name": "销售方",
            "total_amount": "金额",
            "total_tax": "税额",
            "total_with_tax": "价税合计",
        }
        for key, label in field_labels.items():
            f = fields.get(key)
            if f:
                lines.append(f"- {label}: {f.get('value', '')}")

        if validations:
            lines.append("")
            for v in validations:
                icon = "✅" if v.get("passed") else "❌"
                lines.append(f"{icon} {v.get('rule_name', '')}")

        return "\n".join(lines)

    @staticmethod
    def format_html(ocr_result: dict) -> str:
        """Format OCR result as HTML for ERPNext storage."""
        fields = ocr_result.get("fields", {})
        validations = ocr_result.get("validation", [])
        confidence = ocr_result.get("overall_confidence", 0)

        rows = ""
        field_labels = {
            "invoice_code": "发票代码",
            "invoice_number": "发票号码",
            "invoice_date": "开票日期",
            "buyer_name": "购买方",
            "seller_name": "销售方",
            "total_amount": "金额",
            "total_tax": "税额",
            "total_with_tax": "价税合计",
        }
        for key, label in field_labels.items():
            f = fields.get(key)
            if f:
                conf = f.get("confidence", 0)
                color = "green" if conf > 0.8 else ("orange" if conf > 0.6 else "red")
                rows += f'<tr><td style="padding:4px 8px;font-weight:bold">{label}</td><td style="padding:4px 8px">{f.get("value","")}</td><td style="padding:4px 8px;color:{color}">{conf:.0%}</td></tr>'

        checks = ""
        for v in validations:
            icon = "✅" if v.get("passed") else "❌"
            checks += f"<div>{icon} {v.get('rule_name', '')}</div>"

        return f"""<div style="border:1px solid #d1d5db;border-radius:8px;padding:12px;margin:8px 0;background:#f9fafb">
<div style="font-weight:bold;margin-bottom:8px">📄 发票识别结果 <span style="color:#6b7280;font-weight:normal">(置信度: {confidence:.0%})</span></div>
<table style="border-collapse:collapse;width:100%">
<tr style="background:#f3f4f6"><th style="padding:4px 8px;text-align:left">字段</th><th style="padding:4px 8px;text-align:left">值</th><th style="padding:4px 8px;text-align:left">置信度</th></tr>
{rows}
</table>
<div style="margin-top:8px;padding-top:8px;border-top:1px solid #e5e7eb">{checks}</div>
</div>"""

    @staticmethod
    def extract_amount(ocr_result: dict) -> float | None:
        """Extract total_with_tax amount from OCR result."""
        fields = ocr_result.get("fields", {})
        for key in ["total_with_tax", "total_amount"]:
            f = fields.get(key, {})
            value = f.get("value", "")
            if value:
                cleaned = value.replace("¥", "").replace("￥", "").replace(",", "").replace(" ", "").strip()
                try:
                    return float(cleaned)
                except ValueError:
                    continue
        return None
