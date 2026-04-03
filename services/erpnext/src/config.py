from pydantic_settings import BaseSettings, SettingsConfigDict


class LarkSettings(BaseSettings):
    model_config = SettingsConfigDict(env_prefix="LARK_", env_file=".env", extra="ignore")

    app_id: str
    app_secret: str


class ERPNextSettings(BaseSettings):
    model_config = SettingsConfigDict(env_prefix="ERPNEXT_", env_file=".env", extra="ignore")

    url: str
    api_key: str
    api_secret: str


class Settings(BaseSettings):
    model_config = SettingsConfigDict(env_prefix="BRIDGE_", env_file=".env", extra="ignore")

    lark: LarkSettings = LarkSettings()  # type: ignore[call-arg]
    erpnext: ERPNextSettings = ERPNextSettings()  # type: ignore[call-arg]
    expense_approval_code: str = ""
    company: str = ""
    company_abbr: str = ""
    sync_interval_hours: int = 6
