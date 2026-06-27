from urllib.parse import parse_qs, urlparse


def classify_youtube_url(url):
    parsed = urlparse(url)
    query = parse_qs(parsed.query)
    host = parsed.netloc.lower()
    path = parsed.path.strip("/")

    has_playlist = bool(query.get("list")) or path == "playlist"
    has_video = bool(query.get("v"))

    if "youtu.be" in host and path:
        has_video = True
    if path.startswith(("shorts/", "embed/", "live/")):
        has_video = True

    if has_playlist and has_video:
        return "video_in_playlist"
    if has_playlist:
        return "playlist"
    return "single"
