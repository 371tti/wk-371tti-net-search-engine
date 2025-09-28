# -*- coding: utf-8 -*-
"""
簡素版 RSS 逐次クローラ:
 Wikipedia クローラと同じ思想で「1秒1リクエスト (GET/POST 共通)」の RateLimiter を共有。
 並列なし・重複送信許容・加工処理最小。

環境変数:
    ADD_ENDPOINT      (default: https://dev.371tti.net/api/index)
    REQ_INTERVAL      (1リクエスト間隔秒, default 1.0)
    FETCH_INTERVAL    (全フィード1周後スリープ秒, default 600)
    MAX_ENTRIES_PER_FEED (各feedの先頭N件, default 5)
    SUMMARY_MAX       (要約最大長, default 400)

依存:
    pip install aiohttp feedparser
"""
from __future__ import annotations
import os
import asyncio
from typing import List, Dict, Any, Optional
import aiohttp
import feedparser

# Windows 対策が必要ならここで追加 (簡素版では省略)

ADD_ENDPOINT = os.getenv("ADD_ENDPOINT", "https://dev.371tti.net/api/index")
REQ_INTERVAL = float(os.getenv("REQ_INTERVAL", "0.1"))
FETCH_INTERVAL = float(os.getenv("FETCH_INTERVAL", "600"))
MAX_ENTRIES_PER_FEED = int(os.getenv("MAX_ENTRIES_PER_FEED", "8"))
SUMMARY_MAX = int(os.getenv("SUMMARY_MAX", "400"))
USER_AGENT = "RssIngestSimple/0.1 (+https://example.com)"

# --- フィード定義 (最低限: name, url, icon(optional)) ---
FEEDS: List[Dict[str, Any]] = [
    # （必要に応じて減らすと負荷確認が容易）
    {"name": "虚構新聞", "url": "https://kyoko-np.net/index.xml", "icon": "https://kyoko-np.net/images/app.png", "tags": ["news"]},
    {"name": "ギズモード・ジャパン", "url": "https://www.gizmodo.jp/index.xml", "icon": "https://pbs.twimg.com/profile_images/1104329078759280640/N9IqVEvv_400x400.jpg", "tags": ["news","blog"]},
    {"name": "gihyo.jp", "url": "https://gihyo.jp/feed/rss2", "icon": "https://pbs.twimg.com/profile_images/1551442704675983360/9_CHplsd_400x400.png", "tags": ["blog"]},
    *[{"name": f"NHK News cat{i}", "url": f"https://www.nhk.or.jp/rss/news/cat{i}.xml", "icon": "https://i.imgur.com/76KCIrY.png", "tags": ["news"]} for i in range(9)],
    {"name": "3Blue1Brown", "url": "https://3blue1brown.substack.com/feed", "icon": "https://upload.wikimedia.org/wikipedia/commons/thumb/6/64/3B1B_Logo.svg/1200px-3B1B_Logo.svg.png", "plugins": ["unHTML"], "tags": ["academic","blog"]},
    {"name": "fedimagazine.tokyo", "url": "https://fedimagazine.tokyo/feed/", "icon": "https://fedimagazine.tokyo/wp-content/uploads/2023/10/cropped-favicon.png", "tags": ["news"]},
    {"name": "GNU social JP Web", "url": "https://web.gnusocial.jp/feed/", "icon": "https://web.gnusocial.jp/wp-content/uploads/2022/07/cropped-GNU_Social_Image_Logo-4.png", "tags": ["sns"]},
    {"name": "AIDB", "url": "https://ai-data-base.com/feed", "icon": "https://yt3.googleusercontent.com/ytc/AIdro_lf41HCuIWcXUjxiflA2tyVVVMrsnkgJcwySW2r=s176-c-k-c0x00ffffff-no-rj", "plugins": ["unHTML"], "tags": ["news"]},
    {"name": "ゲームビズ", "url": "https://gamebiz.jp/feed.rss", "icon": "https://pbs.twimg.com/profile_images/1507243962662518786/ADct9342_200x200.jpg", "plugins": ["unEscapeHTML"], "tags": ["news"]},
    {"name": "ほのぼの日本史", "url": "https://hono.jp/feed/", "icon": "https://hono.jp/wp-content/uploads/2022/02/100610488_101613778244466_3921142606800617472_n.jpg", "plugins": ["unHTML"], "tags": ["blog","academic"]},
    {"name": "CVE", "url": "https://cvefeed.io/rssfeed/latest.xml", "icon": "https://files.mastodon.social/accounts/avatars/110/947/035/793/757/493/original/4b056135673f8725.png", "plugins": ["unEscapeHTML","unHTML"], "tags": ["news"]},
    {"name": "BlogBooks Library", "url": "https://blogbooks.net/feed", "icon": "https://blogbooks.net/wp-content/uploads/2022/08/logo-2.png", "tags": ["blog"]},
    {"name": "テクニカル諏訪子", "url": "https://technicalsuwako.moe/blog.atom", "icon": "https://technicalsuwako.moe/static/logo.png", "tags": ["blog"]},
    {"name": "Publickey", "url": "https://www.publickey1.jp/atom.xml", "icon": "https://pbs.twimg.com/profile_images/256913586/publickey_icon_400x400.png", "tags": ["news","blog"]},
    {"name": "AdGuard", "url": "https://adguard.com/blog/rss-ja.xml", "icon": "https://upload.wikimedia.org/wikipedia/commons/thumb/4/4c/AdGuard.svg/640px-AdGuard.svg.png", "tags": ["news"]},
    {"name": "特務機関NERV", "url": "https://unnerv.jp/@UN_NERV.rss", "icon": "https://media.unnerv.jp/accounts/avatars/000/000/001/original/d53dd7b3255a6f46.png", "plugins": ["unEscapeHTML","unHTML"], "tags": ["news"]},
    {"name": "akku's website", "url": "https://akku1139.github.io/index.xml", "icon": "https://akku1139.github.io/images/favicon.png", "plugins": ["unEscapeHTML","unHTML"], "tags": ["blog"]},
    {"name": "CoinPost", "url": "https://coinpost.jp/?feed=rsscach32", "icon": "https://coinpost.jp/img/icon.png", "tags": ["news"]},
    {"name": "アニメ！アニメ！", "url": "https://animeanime.jp/rss/index.rdf", "icon": "https://animeanime.jp/base/images/touch-icon-180.png", "tags": ["news"]},
    {"name": "ナショナルジオグラフィック日本版", "url": "https://news.yahoo.co.jp/rss/media/nknatiogeo/all.xml", "icon": "https://s.yimg.jp/images/news/cobranding/nknatiogeo.gif", "tags": ["news"]},
    {"name": "U-Site", "url": "https://u-site.jp/feed", "icon": "https://u-site.jp/wp-content/themes/usite/images/apple-touch-icon.png", "tags": ["blog"]},
    {"name": "電撃ホビーウェブ", "url": "https://hobby.dengeki.com/feed", "icon": "https://hobby.dengeki.com/wp-content/themes/hobby2021/common/img/logo.png", "tags": ["news"]},
    {"name": "CNN.co.jp", "url": "http://feeds.cnn.co.jp/rss/cnn/cnn.rdf", "icon": "https://www.cnn.co.jp/media/cnn/images/common/logo_header_2015.gif", "plugins": ["unEscapeHTML","unHTML"], "tags": ["news"]},
    {"name": "AFPBB News", "url": "http://feeds.afpbb.com/rss/afpbb/afpbbnews", "icon": "https://afpbb.ismcdn.jp/common/images/apple-touch-icon2020.png", "plugins": ["unEscapeHTML","unHTML"], "tags": ["news"]},
    {"name": "IPAセキュリティ情報", "url": "https://www.ipa.go.jp/security/alert-rss.rdf", "icon": "https://www.ipa.go.jp/apple-touch-icon-180x180.png", "tags": ["news"]},
    {"name": "The Keyword", "url": "https://blog.google/rss/", "icon": "https://blog.google/static/blogv2/images/apple-touch-icon.png", "plugins": ["unEscapeHTML","unHTML"], "tags": ["news"]},
    {"name": "JVN", "url": "https://jvn.jp/rss/jvn.rdf", "icon": "https://www.ipa.go.jp/apple-touch-icon-180x180.png", "tags": ["news"]},
    {"name": "アストロアーツ", "url": "https://www.astroarts.co.jp/article/feed.atom", "icon": "https://yt3.googleusercontent.com/ytc/AIdro_lK9Qx3Xa2l5EmrIJ8VkBPK2k7PtGaR4QCc5nFuRYJFUQ=s160-c-k-c0x00ffffff-no-rj", "tags": ["news"]},
    {"name": "AUTOMATON", "url": "https://automaton-media.com/feed/", "icon": "https://automaton-media.com/wp-content/uploads/2024/03/automaton-amp-logo.png", "plugins": ["unHTML"], "tags": ["news"]},
    *[{"name": f"ASCII.jp {n}", "url": f"https://ascii.jp/{k}/rss.xml", "icon": "https://pbs.twimg.com/profile_images/1612620704679329793/N5bSPFFS_400x400.jpg", "plugins": ["unHTML"], "tags": ["news"]} for n,k in [("ビジネス","biz"),("TECH","tech"),("Web Professional","web"),("デジタル","digital"),("iPhone/Mac","mac"),("ゲーム・ホビー","hobby"),("自作PC","pc")]],
    {"name": "ASCII.jp", "url": "https://ascii.jp/rss.xml", "icon": "https://pbs.twimg.com/profile_images/1612620704679329793/N5bSPFFS_400x400.jpg", "plugins": ["unHTML"], "tags": ["news"]},
    *[{"name": f"DistroWatch {n}", "url": f"https://distrowatch.com/news/{k}.xml", "icon": "https://distrowatch.com/images/cpxtu/dwbanner.png", "plugins": ["unEscapeHTML","unHTML"], "tags": ["news"]} for n,k in [("news","dw"),("Latest Distributions","dwd"),("Latest Headlines","news-headlines"),("Packages","dwp")]],
    {"name": "WIRED.jp", "url": "https://wired.jp/feed/rss", "icon": "https://pbs.twimg.com/profile_images/1605821808347082752/aymalKvn_400x400.jpg", "tags": ["news"]},
    {"name": "THE GOLD ONLINE", "url": "https://gentosha-go.com/list/feed/rss", "icon": "https://pbs.twimg.com/profile_images/1685900736021053441/CoHHUCSW_400x400.jpg", "plugins": ["unHTML"], "tags": ["news"]},
    {"name": "エンジニアtype", "url": "https://type.jp/et/feature/feed/", "icon": "https://type.jp/common/img/layout/footer_site_id_logo01.png", "tags": ["news","blog"]},
    {"name": "Japaaan", "url": "https://mag.japaaan.com/feed", "icon": "https://pbs.twimg.com/profile_images/3469257935/0db49db253a2710fd1372b392d595798_400x400.jpeg", "plugins": ["unHTML"], "tags": ["news","blog"]},
    {"name": "withnews", "url": "https://withnews.jp/rss/consumer/new.rdf", "icon": "https://pbs.twimg.com/profile_images/1207550416579252224/oecKIDmH_400x400.jpg", "tags": ["news"]},
    {"name": "ガジェット通信", "url": "https://getnews.jp/feed/ext/orig", "icon": "https://pbs.twimg.com/profile_images/512441585976360960/DMd5at7__400x400.png", "plugins": ["unHTML"], "tags": ["news"]},
    {"name": "現代ビジネス", "url": "https://gendai.media/list/feed/rss", "icon": "https://gendai-m.ismcdn.jp/common/images/v3/logo/cover-logo.png", "tags": ["news"]},
    {"name": "現代農業web", "url": "https://gn.nbkbooks.com/?feed=rss2", "icon": "https://gn.nbkbooks.com/wpblog/wp-content/uploads/2021/11/logo.png", "plugins": ["unHTML"], "tags": ["news"]},
    {"name": "旅する応用言語学", "url": "https://www.nihongo-appliedlinguistics.net/wp/feed", "icon": "https://www.nihongo-appliedlinguistics.net/wp/wp-content/uploads/2021/01/new-logo-150x150.jpg", "tags": ["academic","blog"]},
    {"name": "The Cloudflare Blog", "url": "https://blog.cloudflare.com/rss", "icon": "https://pbs.twimg.com/profile_images/1600539069217480704/RzK50Sks_400x400.jpg", "tags": ["news"]},
    {"name": "xkcd", "url": "https://xkcd.com/atom.xml", "icon": "https://xkcd.com/s/0b7742.png", "plugins": ["unHTML"], "tags": ["blog"]},
    {"name": "PRESIDENT Online", "url": "https://president.jp/list/rss", "icon": "https://president.jp/common/icons/128x128.png", "tags": ["news"]},
    {"name": "Arch Linux News", "url": "https://archlinux.org/feeds/news/", "icon": "https://archlinux.org/static/logos/apple-touch-icon-144x144.png", "plugins": ["unEscapeHTML","unHTML"], "tags": ["news"]},
    {"name": "fabcross", "url": "https://fabcross.jp/rss.xml", "icon": "https://fabcross.jp/images/common/apple-touch-icon-precomposed.png", "tags": ["news"]},
    {"name": "fabcross for エンジニア", "url": "https://engineer.fabcross.jp/smart_format/", "icon": "https://fabcross.jp/images/common/apple-touch-icon-precomposed.png", "plugins": ["unHTML"], "tags": ["news"]},
    {"name": "ホロライブプロダクション", "url": "https://hololivepro.com/news/feed", "icon": "https://pbs.twimg.com/profile_images/1805110423274016768/QSsckQWV_400x400.jpg", "plugins": ["unHTML"], "tags": ["news"]},
    {"name": "萌え萌えmoebuntu", "url": "https://moebuntu.blog.fc2.com/?xml", "icon": "https://moebuntu.web.fc2.com/img/moe_j_logo.png", "tags": ["blog"]},
    {"name": "東方Projectよもやまニュース", "url": "https://touhou-project.news/feed.rss", "icon": "https://i.imgur.com/yjwXFbN.png", "plugins": ["unEscapeHTML"], "tags": ["news"]},
    {"name": "ダイヤモンド・オンライン", "url": "https://diamond.jp/list/feed/rss/dol", "icon": "https://pbs.twimg.com/profile_images/1355858337825386500/dN6N0nUi_400x400.jpg", "tags": ["news"]},
    {"name": "TechFeed", "url": "https://techfeed.io/feeds/original-contents", "icon": "https://play-lh.googleusercontent.com/lpVgh0bGMLPnIIjMvlsoMlSsPmkfQBBlr4kBgYUQOsnhaE3tE04jd7E-W-_XRXtVVLL2=w240-h480", "tags": ["news"]},
    {"name": "CodeZine", "url": "https://codezine.jp/rss/new/index.xml", "icon": "https://pbs.twimg.com/profile_images/1267291016035307522/OEH0rwXO_400x400.jpg", "tags": ["news"]},
    {"name": "Engadget", "url": "https://www.engadget.com/rss.xml", "icon": "https://s.yimg.com/kw/assets/apple-touch-icon-152x152.png", "plugins": ["unHTML"], "tags": ["news"]},
    {"name": "TechnoEdge", "url": "https://www.techno-edge.net/rss20/index.rdf", "icon": "https://pbs.twimg.com/profile_images/1650368239082541056/3JriLBez_400x400.jpg", "tags": ["news"]},
    {"name": "価格.com マガジン", "url": "https://kakakumag.com/rss/", "icon": "https://pbs.twimg.com/profile_images/877052835182936064/yNkw85sy_400x400.jpg", "tags": ["news"]},
    *[{"name": f"聯合ニュース {n}", "url": f"https://jp.yna.co.kr/RSS/{k}.xml", "icon": "https://r.yna.co.kr/global/home/v01/img/favicon-152.png", "plugins": ["cleanAllURLParams"], "tags": ["news"]} for n,k in [("政治","politics"),("北朝鮮","nk"),("韓日関係","japan-relationship"),("経済","economy"),("社会・文化","society-culture"),("IT・科学","it-science"),("芸能・スポーツ","entertainment-sports"),("全般","news")]],
    {"name": "アリエナイ理科ポータル", "url": "https://www.cl20.jp/portal/feed/", "icon": "https://www.cl20.jp/portal/wp-content/uploads/2018/11/cropped-favicon-192x192.png", "plugins": ["unEscapeHTML"], "tags": ["blog"]},
    {"name": "GAZLOG", "url": "https://gazlog.jp/feed/", "icon": "https://gazlog.jp/wp-content/uploads/2024/02/cropped-Gazlog-favcon-3-1-192x192.jpg", "plugins": ["unHTML"], "tags": ["blog"]},
    {"name": "アナログ(4Gamer tag)", "url": "https://www.4gamer.net/tags/TS/TS020/contents.xml", "icon": "https://pbs.twimg.com/profile_images/1452883854914560002/RD2jcwNm_400x400.png", "tags": ["news"]},
    {"name": "Sysdig", "url": "https://sysdig.jp/feed/", "icon": "https://sysdig.jp/wp-content/uploads/favicon-350x350.png", "plugins": ["unHTML"], "tags": ["news"]},
    {"name": "JPCERT/CC Eyes", "url": "https://blogs.jpcert.or.jp/ja/atom.xml", "icon": "https://pbs.twimg.com/profile_images/882458634629795840/osK0iO8z_400x400.jpg", "tags": ["news"]},
    {"name": "DevelopersIO", "url": "https://dev.classmethod.jp/feed/", "icon": "https://i.imgur.com/ryh2cVZ.png", "tags": ["news","blog"]},
    {"name": "XenoSpectrum", "url": "https://xenospectrum.com/feed/", "icon": "https://xenospectrum.com/wp-content/uploads/2024/03/xs-logo-300x300.png", "plugins": ["unHTML"], "tags": ["blog"]},
    {"name": "TechPowerUp News", "url": "https://www.techpowerup.com/rss/news", "icon": "https://tpucdn.com/apple-touch-icon-v1728765512776.png", "plugins": ["unHTML"], "tags": ["news"]},
    {"name": "XDA", "url": "https://www.xda-developers.com/feed/", "icon": "https://www.xda-developers.com/public/build/images/favicon-240x240.43161a66.png", "plugins": ["unHTML"], "tags": ["news"]},
    {"name": "Phoronix", "url": "https://www.phoronix.com/rss.php", "icon": "https://www.phoronix.com/android-chrome-192x192.png", "tags": ["news"]},
    {"name": "電ファミニコゲーマー", "url": "https://news.denfaminicogamer.jp/feed", "icon": "https://news.denfaminicogamer.jp/wp-content/uploads/2016/12/apple-touch-icon.png", "tags": ["news"]},
    {"name": "EE Times Japan", "url": "https://rss.itmedia.co.jp/rss/2.0/eetimes.xml", "icon": "https://pbs.twimg.com/profile_images/1591982815146803200/8yMm3WAW_400x400.png", "tags": ["news"]},
    {"name": "探査報道", "url": "https://tansajp.org/investigativejournal_oneshot/one_shot/feed/", "icon": "https://pbs.twimg.com/profile_images/1368745066764722177/5dsSuMY6_400x400.jpg", "plugins": ["unHTML"], "tags": ["news"]},
    {"name": "ニュース - tansajp", "url": "https://tansajp.org/investigativejournal/feed/", "icon": "https://pbs.twimg.com/profile_images/1368745066764722177/5dsSuMY6_400x400.jpg", "plugins": ["unHTML"], "tags": ["news"]},
    {"name": "Latest Linux Kernel Versions", "url": "https://www.kernel.org/feeds/kdist.xml", "icon": "https://www.kernel.org/theme/images/logos/tux.png", "plugins": ["unEscapeHTML","unHTML","linuxReleaseID"], "tags": ["news"]},
    {"name": "ROM焼き試験場", "url": "https://mitanyan98.hatenablog.com/feed", "icon": "https://cdn.blog.st-hatena.com/images/theme/og-image-1500.png", "plugins": ["unEscapeHTML","unHTML"], "tags": ["blog"]},
    {"name": "postmarketOS Blog", "url": "https://postmarketos.org/blog/feed.atom", "icon": "https://upload.wikimedia.org/wikipedia/commons/thumb/a/a6/PostmarketOS_logo.svg/150px-PostmarketOS_logo.svg.png", "plugins": ["unEscapeHTML","unHTML"], "tags": ["news"]},
    {"name": "お知らせ - 東方LostWord", "url": "https://touhoulostword.com/feed/", "icon": "https://touhoulostword.com/assets/images/og_image.png", "plugins": ["unHTML"], "tags": ["news"]},
    {"name": "すまほん!!", "url": "https://smhn.info/feed", "icon": "https://smhn.info/wp-content/themes/confidence/images/favicon-big.png", "plugins": ["unHTML"], "tags": ["news"]},
]

class RateLimiter:
    def __init__(self, interval: float):
        self.interval = interval
        self._last = 0.0
        self._lock = asyncio.Lock()
    async def wait(self):
        async with self._lock:
            now = asyncio.get_event_loop().time()
            sleep_for = self.interval - (now - self._last)
            if sleep_for > 0:
                await asyncio.sleep(sleep_for)
            self._last = asyncio.get_event_loop().time()

rate_limiter = RateLimiter(REQ_INTERVAL)

# --- HTTP ---
async def fetch_feed(session: aiohttp.ClientSession, feed: Dict[str, Any]) -> Optional[feedparser.FeedParserDict]:
    await rate_limiter.wait()
    try:
        async with session.get(feed['url'], timeout=30, headers={"User-Agent": USER_AGENT}) as resp:
            if resp.status != 200:
                print(f"[WARN] {feed['name']} status={resp.status}")
                return None
            raw = await resp.read()
    except Exception as e:
        print(f"[ERR] fetch {feed['name']}: {e}")
        return None
    return feedparser.parse(raw)
DEFAULT_TAGS = ["news"]  # feed に tags が無い場合のみ使用

async def post_entry(session: aiohttp.ClientSession, feed: Dict[str, Any], entry: feedparser.FeedParserDict):
    # GET と POST 共通で 1 秒間隔
    title = (entry.get('title') or '').strip()
    link = (entry.get('link') or '').strip()
    if not link:
        return
    summary = ''
    # summary / content
    if 'summary' in entry:
        summary = entry.summary
    elif 'content' in entry:
        try:
            summary = entry.content[0].value
        except Exception:
            pass
    summary = summary.strip()
    if not title:
        title = link
    summary_proc = (summary or '')[:SUMMARY_MAX]
    tags = feed.get('tags') or DEFAULT_TAGS
    payload = {"url": link, "title": None, "favicon": None, "tags": tags, "descriptions": summary_proc}
    await rate_limiter.wait()
    try:
        async with session.post(ADD_ENDPOINT, json=payload, timeout=30, headers={"User-Agent": USER_AGENT}) as resp:
            if resp.status >= 300:
                txt = (await resp.text())[:60]
                print(f"[WARN] add {resp.status} {title[:38]} {txt}")
            else:
                print(f"[OK] {feed['name']} :: {title[:60]}")
    except Exception as e:
        print(f"[ERR] post {title[:38]} {e}")

async def process_feed(session: aiohttp.ClientSession, feed: Dict[str, Any]):
    parsed = await fetch_feed(session, feed)
    if not parsed:
        return
    for e in parsed.entries[:MAX_ENTRIES_PER_FEED]:
        await post_entry(session, feed, e)

async def main_loop():
    print(f"[INFO] start rss simple crawler 1req/{REQ_INTERVAL:.1f}s feeds={len(FEEDS)}")
    async with aiohttp.ClientSession(headers={"User-Agent": USER_AGENT}) as session:
        loop = 0
        while True:
            loop += 1
            print(f"[LOOP {loop}] begin")
            for feed in FEEDS:
                await process_feed(session, feed)
            print(f"[SLEEP] {FETCH_INTERVAL}s")
            await asyncio.sleep(FETCH_INTERVAL)

if __name__ == '__main__':
    try:
        asyncio.run(main_loop())
    except KeyboardInterrupt:
        print("\n[INFO] stopped")
