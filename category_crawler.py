import os
import asyncio
import aiohttp
import urllib.parse
from datetime import datetime

CATEGORY = os.getenv("CATEGORY", "数学に関する記事")
WIKI_API = "https://ja.wikipedia.org/w/api.php"
REST_SUMMARY = "https://ja.wikipedia.org/api/rest_v1/page/summary/"
PAGE_URL_PREFIX = "https://ja.wikipedia.org/wiki/"
SEARCH_ENGINE_ADD = os.getenv("SEARCH_ADD_ENDPOINT", "http://localhost:90/add")
USER_AGENT = f"CategoryCrawler/ja 0.1 (+https://example.com)"
REQ_INTERVAL = float(os.getenv("REQ_INTERVAL", "0.0"))
SUMMARY_MAX = int(os.getenv("SUMMARY_MAX", "800"))

class RateLimiter:
    def __init__(self, interval: float):
        self.interval = interval
        self._last = 0.0
        self._lock = asyncio.Lock()

    async def wait(self):
        async with self._lock:
            loop_time = asyncio.get_event_loop().time()
            delta = loop_time - self._last
            sleep_for = self.interval - delta
            if sleep_for > 0:
                await asyncio.sleep(sleep_for)
            self._last = asyncio.get_event_loop().time()

rate_limiter = RateLimiter(REQ_INTERVAL)

async def http_json(session, url, params=None, timeout=30):
    await rate_limiter.wait()
    try:
        async with session.get(url, params=params, timeout=timeout) as resp:
            if resp.status != 200:
                return None
            return await resp.json()
    except Exception:
        return None

async def http_post_json(session, url, json_payload, timeout=30):
    await rate_limiter.wait()
    try:
        async with session.post(url, json=json_payload, timeout=timeout) as resp:
            text = await resp.text()
            return resp.status, text
    except Exception as e:
        return 599, str(e)

async def get_category_pages(session, category, limit=10000):
    pages = []
    ccontinue = None
    while True:
        params = {
            "action": "query",
            "list": "categorymembers",
            "cmtitle": f"Category:{category}",
            "cmnamespace": 0,
            "cmlimit": "500",
            "format": "json"
        }
        if ccontinue:
            params["cmcontinue"] = ccontinue
        data = await http_json(session, WIKI_API, params=params)
        if not data:
            break
        members = data.get("query", {}).get("categorymembers", [])
        for m in members:
            title = m.get("title")
            if title:
                pages.append(title)
                if len(pages) >= limit:
                    return pages
        ccontinue = data.get("continue", {}).get("cmcontinue")
        if not ccontinue:
            break
    return pages

async def fetch_summary(session, title):
    url = REST_SUMMARY + urllib.parse.quote(title, safe="")
    data = await http_json(session, url)
    if not data or "extract" not in data:
        return None
    return data

async def post_document(session, url, title, extract):
    payload = {
        "url": url,
        "title": title,
        "favicon": None,
        "tags": ["wiki"],
        "descriptions": extract,
        "target_selector": ".mw-body-content"
    }
    status, text = await http_post_json(session, SEARCH_ENGINE_ADD, payload)
    if status >= 300:
        print(f"[WARN] add {status} {title[:30]} {text[:60]}")
    else:
        print(f"[OK] {title[:40]}")

async def main():
    print(f"[INFO] start category crawler: {CATEGORY}")
    async with aiohttp.ClientSession(headers={"User-Agent": USER_AGENT}) as session:
        print("[INFO] loading category pages...")
        titles = await get_category_pages(session, CATEGORY)
        print(f"[INFO] got {len(titles)} pages.")
        for idx, title in enumerate(titles, 1):
            summary = await fetch_summary(session, title)
            if not summary:
                print(f"[SKIP] {title}")
                continue
            extract = (summary.get("extract") or "").strip()
            if not extract:
                print(f"[SKIP] {title} (no extract)")
                continue
            page_url = PAGE_URL_PREFIX + urllib.parse.quote(title.replace(" ", "_"))
            await post_document(session, page_url, summary.get("title") or title, extract[:SUMMARY_MAX])
            print(f"[PROGRESS] {idx}/{len(titles)}")

if __name__ == "__main__":
    try:
        asyncio.run(main())
    except KeyboardInterrupt:
        print("\n[INFO] stopped")
