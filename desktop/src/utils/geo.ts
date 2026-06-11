// Shared geo-alias table + node-name → location heuristic. Used by both the
// node sidebar (groups by country) and the world map (city dots).
//
// Node names from the subscribe usually look like
//   "🇯🇵 日本-东京-IPLC-01" / "Tokyo HKT-VLESS-04" / "美国洛杉矶 直连 1x"
// so we walk an alias list of {country, city, lat, lon, keys[]} and match on
// the first keyword that appears. The order matters — city aliases should
// come before generic country aliases so "东京" resolves to Tokyo (35.67)
// not generic Japan (36.20).

export interface GeoPoint {
  label: string;
  country: string;
  flag: string;
  lat: number;
  lon: number;
}

export interface GeoAlias extends GeoPoint {
  keys: string[];
}

export const GEO_ALIASES: GeoAlias[] = [
  { label: "Taipei", country: "台湾", flag: "🇹🇼", lat: 25.033, lon: 121.565, keys: ["台北", "taipei"] },
  { label: "Taiwan", country: "台湾", flag: "🇹🇼", lat: 23.697, lon: 120.961, keys: ["台湾", "台灣", "taiwan", " tw "] },
  { label: "Tokyo", country: "日本", flag: "🇯🇵", lat: 35.676, lon: 139.65, keys: ["东京", "東京", "tokyo"] },
  { label: "Osaka", country: "日本", flag: "🇯🇵", lat: 34.694, lon: 135.502, keys: ["大阪", "osaka"] },
  { label: "Japan", country: "日本", flag: "🇯🇵", lat: 36.204, lon: 138.253, keys: ["日本", "japan", " jp "] },
  { label: "Hong Kong", country: "香港", flag: "🇭🇰", lat: 22.319, lon: 114.169, keys: ["香港", "hong kong", "hongkong", " hk "] },
  { label: "Singapore", country: "新加坡", flag: "🇸🇬", lat: 1.352, lon: 103.82, keys: ["新加坡", "singapore", " sg "] },
  { label: "Seoul", country: "韩国", flag: "🇰🇷", lat: 37.566, lon: 126.978, keys: ["首尔", "首爾", "seoul"] },
  { label: "Korea", country: "韩国", flag: "🇰🇷", lat: 36.5, lon: 127.8, keys: ["韩国", "韓國", "korea", " kr "] },
  { label: "Los Angeles", country: "美国", flag: "🇺🇸", lat: 34.052, lon: -118.244, keys: ["洛杉矶", "洛杉磯", "los angeles", "la-"] },
  { label: "San Jose", country: "美国", flag: "🇺🇸", lat: 37.338, lon: -121.886, keys: ["圣何塞", "聖何塞", "san jose", "sanjose"] },
  { label: "New York", country: "美国", flag: "🇺🇸", lat: 40.713, lon: -74.006, keys: ["纽约", "紐約", "new york"] },
  { label: "United States", country: "美国", flag: "🇺🇸", lat: 39.828, lon: -98.579, keys: ["美国", "美國", "united states", "usa", " us "] },
  { label: "London", country: "英国", flag: "🇬🇧", lat: 51.507, lon: -0.128, keys: ["伦敦", "倫敦", "london"] },
  { label: "United Kingdom", country: "英国", flag: "🇬🇧", lat: 54.0, lon: -2.0, keys: ["英国", "英國", "united kingdom", " uk "] },
  { label: "Frankfurt", country: "德国", flag: "🇩🇪", lat: 50.11, lon: 8.682, keys: ["法兰克福", "法蘭克福", "frankfurt"] },
  { label: "Germany", country: "德国", flag: "🇩🇪", lat: 51.165, lon: 10.452, keys: ["德国", "德國", "germany", " de "] },
  { label: "Paris", country: "法国", flag: "🇫🇷", lat: 48.857, lon: 2.352, keys: ["巴黎", "paris", "法国", "法國", "france"] },
  { label: "Amsterdam", country: "荷兰", flag: "🇳🇱", lat: 52.367, lon: 4.904, keys: ["阿姆斯特丹", "amsterdam", "荷兰", "荷蘭", "netherlands", " nl "] },
  { label: "Sydney", country: "澳大利亚", flag: "🇦🇺", lat: -33.869, lon: 151.209, keys: ["悉尼", "sydney"] },
  { label: "Australia", country: "澳大利亚", flag: "🇦🇺", lat: -25.274, lon: 133.775, keys: ["澳大利亚", "澳洲", "australia", " au "] },
  { label: "Toronto", country: "加拿大", flag: "🇨🇦", lat: 43.653, lon: -79.383, keys: ["多伦多", "多倫多", "toronto"] },
  { label: "Canada", country: "加拿大", flag: "🇨🇦", lat: 56.13, lon: -106.347, keys: ["加拿大", "canada", " ca "] },
  { label: "Bangkok", country: "泰国", flag: "🇹🇭", lat: 13.756, lon: 100.501, keys: ["曼谷", "bangkok", "泰国", "泰國", "thailand"] },
  { label: "Ho Chi Minh City", country: "越南", flag: "🇻🇳", lat: 10.823, lon: 106.63, keys: ["胡志明", "越南", "vietnam", " vn "] },
  { label: "Manila", country: "菲律宾", flag: "🇵🇭", lat: 14.599, lon: 120.984, keys: ["马尼拉", "馬尼拉", "菲律宾", "菲律賓", "philippines"] },
  { label: "Kuala Lumpur", country: "马来西亚", flag: "🇲🇾", lat: 3.139, lon: 101.687, keys: ["吉隆坡", "马来", "馬來", "malaysia"] },
  { label: "Jakarta", country: "印尼", flag: "🇮🇩", lat: -6.208, lon: 106.846, keys: ["雅加达", "雅加達", "印尼", "indonesia"] },
  { label: "Mumbai", country: "印度", flag: "🇮🇳", lat: 19.076, lon: 72.878, keys: ["孟买", "孟買", "mumbai"] },
  { label: "India", country: "印度", flag: "🇮🇳", lat: 20.594, lon: 78.963, keys: ["印度", "india"] },
  { label: "Dubai", country: "阿联酋", flag: "🇦🇪", lat: 25.205, lon: 55.271, keys: ["迪拜", "dubai", "阿联酋", "阿聯酋", "uae"] },
  { label: "Istanbul", country: "土耳其", flag: "🇹🇷", lat: 41.008, lon: 28.978, keys: ["伊斯坦布尔", "土耳其", "turkey", "istanbul"] },
  { label: "Moscow", country: "俄罗斯", flag: "🇷🇺", lat: 55.756, lon: 37.617, keys: ["莫斯科", "俄罗斯", "俄羅斯", "russia"] },
  { label: "Shanghai", country: "中国", flag: "🇨🇳", lat: 31.231, lon: 121.474, keys: ["上海", "shanghai"] },
  { label: "Beijing", country: "中国", flag: "🇨🇳", lat: 39.904, lon: 116.407, keys: ["北京", "beijing", "中国", "中國", "china"] },
  { label: "Macau", country: "澳门", flag: "🇲🇴", lat: 22.199, lon: 113.544, keys: ["澳门", "澳門", "macau", "macao"] },
];

export function normalizeNodeName(name: string): string {
  return ` ${name
    .toLowerCase()
    .replace(/[|｜_\-·•/()[\]{}]+/g, " ")
    .replace(/\s+/g, " ")} `;
}

export function locateNode(name: string): GeoPoint | null {
  const normalized = normalizeNodeName(name);
  const hit = GEO_ALIASES.find((entry) =>
    entry.keys.some((key) => normalized.includes(key.toLowerCase())),
  );
  if (!hit) return null;
  const { keys: _keys, ...point } = hit;
  return point;
}
