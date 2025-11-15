

import os
import asyncio
import aiohttp

INPUT_FILE = 'urls.txt'  # 1行1URL or 1行: url[,title[,descriptions]]
SEARCH_ENGINE_ADD = os.getenv('SEARCH_ADD_ENDPOINT', 'http://localhost:90/add')
USER_AGENT = 'UrlListAddAsync/0.1 (+https://example.com)'

def parse_line(line):
    # url[,title[,descriptions]]
    parts = [p.strip() for p in line.split(',', 2)]
    url = parts[0] if len(parts) > 0 else ''
    title = parts[1] if len(parts) > 1 else None
    descriptions = parts[2] if len(parts) > 2 else None
    return url, title, descriptions

async def post_url(session: aiohttp.ClientSession, url: str, title=None, descriptions=None):
    payload = {'url': url}
    if title:
        payload['title'] = title
    if descriptions:
        payload['descriptions'] = descriptions
    try:
        async with session.post(SEARCH_ENGINE_ADD, json=payload, timeout=10) as resp:
            text = await resp.text()
            if resp.status >= 300:
                print(f"[WARN] add {resp.status} {url[:40]} {text[:60]}")
            else:
                print(f"[OK] {url[:40]}")
    except Exception as e:
        print(f"[ERR] {url[:40]} {e}")

async def main():
    with open(INPUT_FILE, encoding='utf-8') as f:
        lines = [line.strip() for line in f if line.strip()]
    async with aiohttp.ClientSession(headers={'User-Agent': USER_AGENT}) as session:
        for line in lines:
            url, title, descriptions = parse_line(line)
            if not url:
                continue
            print(f'Add: {url}')
            await post_url(session, url, title, descriptions)
            await asyncio.sleep(0.1)  # 過負荷防止

if __name__ == '__main__':
    asyncio.run(main())
