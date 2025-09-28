# -*- coding: utf-8 -*-
"""
非同期版: すべての API リクエストを 1 秒に 1 回だけ発行する軽量クローラ
(秀逸/良質/トップビュー/ランダムを巡回。重複送信許容)
"""

import os
import random
import urllib.parse
import asyncio
from datetime import datetime, timedelta
from collections import deque
import aiohttp
from aiohttp import ClientSession

LANG = os.getenv("WIKI_LANG", "ja")
PROJECT = f"{LANG}.wikipedia"
WIKI_API = f"https://{LANG}.wikipedia.org/w/api.php"
REST_SUMMARY = f"https://{LANG}.wikipedia.org/api/rest_v1/page/summary/"
PAGE_URL_PREFIX = f"https://{LANG}.wikipedia.org/wiki/"

SEARCH_ENGINE_ADD = os.getenv("SEARCH_ADD_ENDPOINT", "https://dev.371tti.net/api/index")
BATCH_FEATURED = int(os.getenv("BATCH_FEATURED", "0"))
BATCH_GOOD = int(os.getenv("BATCH_GOOD", "0"))
BATCH_TOPVIEW = int(os.getenv("BATCH_TOPVIEW", "0"))
BATCH_RANDOM = int(os.getenv("BATCH_RANDOM", "24"))
BATCH_MAX = int(os.getenv("BATCH_MAX", "24"))

LOOP_SLEEP = float(os.getenv("LOOP_SLEEP", "1"))      # 1 メインループ後の待機
SUMMARY_MAX = int(os.getenv("SUMMARY_MAX", "800"))
TOPVIEW_REFRESH = int(os.getenv("TOPVIEW_REFRESH", "3600"))

REQ_INTERVAL = float(os.getenv("REQ_INTERVAL", "0.1"))  # 1秒/リクエスト

USER_AGENT = f"WikiUsefulCrawlerAsync/{LANG} 0.1 (+https://example.com; mailto:you@example.com)"


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


async def http_json(session: ClientSession, url: str, params=None, timeout=30):
    await rate_limiter.wait()
    try:
        async with session.get(url, params=params, timeout=timeout) as resp:
            if resp.status != 200:
                return None
            return await resp.json()
    except Exception:
        return None


async def http_post_json(session: ClientSession, url: str, json_payload: dict, timeout=30):
    await rate_limiter.wait()
    try:
        async with session.post(url, json=json_payload, timeout=timeout) as resp:
            text = await resp.text()
            return resp.status, text
    except Exception as e:
        return 599, str(e)


async def get_category_pages(session: ClientSession, category: str, limit: int = 10000):
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


async def get_random_pages(session: ClientSession, n: int):
    params = {
        "action": "query",
        "list": "random",
        "rnnamespace": 0,
        "rnlimit": str(n),
        "format": "json"
    }
    data = await http_json(session, WIKI_API, params=params)
    if not data:
        return []
    return [r["title"] for r in data.get("query", {}).get("random", []) if r.get("title")]


async def get_topview_titles(session: ClientSession):
    day = (datetime.utcnow() - timedelta(days=1)).strftime("%Y/%m/%d")
    url = f"https://wikimedia.org/api/rest_v1/metrics/pageviews/top/{PROJECT}/all-access/{day}"
    data = await http_json(session, url)
    if not data:
        return []
    articles = data.get("items", [{}])[0].get("articles", [])
    titles = []
    for a in articles:
        t = a.get("article")
        if not t:
            continue
        t = t.replace("_", " ")
        if ':' in t:
            continue
        titles.append(t)
    return titles


async def fetch_summary(session: ClientSession, title: str):
    url = REST_SUMMARY + urllib.parse.quote(title, safe="")
    data = await http_json(session, url)
    if not data or "extract" not in data:
        return None
    return data


async def post_document(session: ClientSession, url: str, title: str, extract: str):
    payload = {
        "url": url,
        "title": None,
        "favicon": None,
        "tags": ["wiki"],
        "descriptions": extract
    }
    status, text = await http_post_json(session, SEARCH_ENGINE_ADD, payload)
    if status >= 300:
        print(f"[WARN] add {status} {title[:30]} {text[:60]}")
    else:
        print(f"[OK] {title[:40]}")


def cycle_deque(dq: deque, count: int):
    out = []
    for _ in range(min(count, len(dq))):
        v = dq.popleft()
        out.append(v)
        dq.append(v)
    return out


async def main():
    print(f"[INFO] start async crawler lang={LANG} (1req/{REQ_INTERVAL:.1f}s)")

    async with aiohttp.ClientSession(headers={"User-Agent": USER_AGENT}) as session:
        print("[INFO] loading categories (時間がかかる可能性あり)")
        featured_list, good_list = await asyncio.gather(
            get_category_pages(session, "秀逸な記事"),
            get_category_pages(session, "良質な記事"),
        )
        random.shuffle(featured_list)
        random.shuffle(good_list)

        featured_dq = deque(featured_list)
        good_dq = deque(good_list)
        topview_cache = []
        topview_time = 0.0
        loop = 0

        while True:
            loop += 1
            now = asyncio.get_event_loop().time()
            wall_now = datetime.utcnow()

            # Topview 更新
            if (now - topview_time) > TOPVIEW_REFRESH or not topview_cache:
                tv = await get_topview_titles(session)
                if tv:
                    random.shuffle(tv)
                    topview_cache = tv
                    topview_time = now
                    print(f"[INFO] refresh topview count={len(tv)}")

            titles = []
            if featured_dq:
                titles += cycle_deque(featured_dq, BATCH_FEATURED)
            if good_dq:
                titles += cycle_deque(good_dq, BATCH_GOOD)
            if topview_cache:
                take = min(BATCH_TOPVIEW, len(topview_cache))
                for _ in range(take):
                    titles.append(topview_cache.pop())
            if len(titles) < BATCH_MAX:
                need = BATCH_MAX - len(titles)
                titles += await get_random_pages(session, need)

            # 重複削減 (必須ではない)
            seen_local = set()
            uniq = []
            for t in titles:
                if t not in seen_local:
                    uniq.append(t)
                    seen_local.add(t)

            if not uniq:
                print("[WARN] no titles; sleeping")
                await asyncio.sleep(LOOP_SLEEP)
                continue

            print(f"[LOOP {loop}] utc={wall_now.isoformat()} process={len(uniq)}")

            # シリアル処理: summary → add (各2リクエスト → 約2秒/記事)
            for title in uniq:
                summary = await fetch_summary(session, title)
                if not summary:
                    continue
                extract = (summary.get("extract") or "").strip()
                if not extract:
                    continue
                page_url = PAGE_URL_PREFIX + urllib.parse.quote(title.replace(" ", "_"))
                await post_document(session, page_url, summary.get("title") or title, extract[:SUMMARY_MAX])

            await asyncio.sleep(LOOP_SLEEP)


if __name__ == "__main__":
    try:
        asyncio.run(main())
    except KeyboardInterrupt:
        print("\n[INFO] stopped")