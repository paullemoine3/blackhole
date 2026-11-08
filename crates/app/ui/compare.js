// Run in a tab by the comparator ("Compare tabs"): what the page says about the product it
// shows, from its structured data (schema.org JSON-LD, then microdata and Open Graph), plus the
// rows of its first specification table. Returns JSON; reads only, changes nothing.
(() => {
  const text = (v) => (v == null ? '' : String(v).replace(/\s+/g, ' ').trim()).slice(0, 120);
  const meta = (k) => {
    const el = document.querySelector(`meta[property="${k}"],meta[name="${k}"],meta[itemprop="${k}"]`);
    return el ? text(el.getAttribute('content')) : '';
  };
  const out = { title: text(document.title), name: '', price: '', currency: '', rating: '', reviews: '', brand: '', availability: '', specs: {} };
  // JSON-LD: the first Product found, wherever it sits (@graph, arrays, nested).
  const products = [];
  const walk = (node, depth) => {
    if (!node || typeof node !== 'object' || depth > 6) return;
    if (Array.isArray(node)) return node.forEach((n) => walk(n, depth + 1));
    const type = [].concat(node['@type'] || []).join(' ');
    if (/Product/.test(type)) products.push(node);
    for (const k of ['@graph', 'mainEntity', 'itemListElement', 'item']) walk(node[k], depth + 1);
  };
  for (const s of document.querySelectorAll('script[type="application/ld+json"]')) {
    try { walk(JSON.parse(s.textContent), 0); } catch (_) {}
  }
  const p = products[0];
  if (p) {
    const offer = [].concat(p.offers || [])[0] || {};
    const first = (o) => [].concat(o || [])[0] || {};
    out.name = text(p.name);
    out.price = text(offer.price ?? offer.lowPrice ?? first(offer.offers).price);
    out.currency = text(offer.priceCurrency ?? first(offer.offers).priceCurrency);
    out.availability = text(String(offer.availability || '').replace(/^https?:\/\/schema\.org\//, ''));
    out.rating = text(p.aggregateRating && p.aggregateRating.ratingValue);
    out.reviews = text(p.aggregateRating && (p.aggregateRating.reviewCount ?? p.aggregateRating.ratingCount));
    out.brand = text(typeof p.brand === 'object' ? p.brand && p.brand.name : p.brand);
  }
  out.name = out.name || meta('og:title');
  out.price = out.price || meta('product:price:amount') || meta('og:price:amount') || meta('price');
  out.currency = out.currency || meta('product:price:currency') || meta('og:price:currency') || meta('priceCurrency');
  out.brand = out.brand || meta('product:brand') || meta('brand');
  // Specifications: two-cell rows of the first table that has some.
  for (const table of document.querySelectorAll('table')) {
    let n = 0;
    for (const row of table.rows) {
      const cells = row.cells;
      if (cells.length !== 2) continue;
      const k = text(cells[0].innerText), v = text(cells[1].innerText);
      if (k && v && k.length < 50 && !(k in out.specs)) { out.specs[k] = v; n++; }
      if (n >= 25) break;
    }
    if (n >= 3) break;
    out.specs = {};
  }
  return JSON.stringify(out);
})()
