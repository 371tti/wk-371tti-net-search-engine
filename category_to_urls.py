import aiohttp
import asyncio
import urllib.parse

CATEGORY = "コンピュータ関連のスタブ項目"
WIKI_API = "https://ja.wikipedia.org/w/api.php"
PAGE_URL_PREFIX = "https://ja.wikipedia.org/wiki/"
OUTPUT_FILE = "urls.txt"

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

async def http_json(session, url, params=None, timeout=30):
    try:
        async with session.get(url, params=params, timeout=timeout) as resp:
            if resp.status != 200:
                return None
            return await resp.json()
    except Exception:
        return None

async def main():
    async with aiohttp.ClientSession() as session:
        print(f"[INFO] Fetching pages in category: {CATEGORY}")
        pages = await get_category_pages(session, CATEGORY)
        print(f"[INFO] Got {len(pages)} pages.")
        with open(OUTPUT_FILE, "w", encoding="utf-8") as f:
            for title in pages:
                url = PAGE_URL_PREFIX + urllib.parse.quote(title.replace(' ', '_'))
                f.write(url + "\n")
        print(f"[INFO] URLs written to {OUTPUT_FILE}")

if __name__ == "__main__":
    asyncio.run(main())
