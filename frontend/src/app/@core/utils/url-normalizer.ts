export function normalizeUrl(url: string): string {
  try {
    let decoded = url;
    try {
      decoded = decodeURIComponent(url);
    } catch (e) {
      decoded = url;
    }

    const [path, queryString] = decoded.split('?');

    let normalizedPath = path.replace(/\/+/g, '/').replace(/\/+$/, '');
    if (!normalizedPath.startsWith('/')) {
      normalizedPath = '/' + normalizedPath;
    }

    if (queryString) {
      try {
        const params = new URLSearchParams(queryString);
        const pairs: string[][] = [];
        params.forEach((value, key) => pairs.push([key, value]));
        pairs.sort((a, b) => a[0].localeCompare(b[0]));
        const normalizedParams = new URLSearchParams(pairs);
        return normalizedPath + '?' + normalizedParams.toString();
      } catch (e) {
        return normalizedPath + '?' + queryString;
      }
    }

    return normalizedPath;
  } catch (e) {
    return url;
  }
}
