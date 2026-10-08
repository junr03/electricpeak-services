(() => {
  const visible = e => !!(e.getBoundingClientRect().width || e.getBoundingClientRect().height);
  const locked = [...document.querySelectorAll('[class*="paywall" i], [aria-label="Paywall"]')].some(visible);
  const element = document.querySelector('.available-content .body.markup, .body.markup');
  if (!element || !visible(element)) return JSON.stringify({missing: true});
  const root = element.cloneNode(true);
  const originals = element.querySelectorAll('img');
  root.querySelectorAll('img').forEach((image, i) => {
    image.setAttribute('src', originals[i].currentSrc || originals[i].src);
  });
  root.querySelectorAll('script,style,meta,base,iframe,form,button,input,video,audio,svg,link,object,embed').forEach(e => e.remove());
  root.querySelectorAll('*').forEach(e => {
    for (const a of [...e.attributes]) {
      if (!['href','src','alt','colspan','rowspan'].includes(a.name)) e.removeAttribute(a.name);
    }
    for (const name of ['href','src']) {
      if (e.hasAttribute(name)) {
        const url = new URL(e.getAttribute(name), document.baseURI);
        if (url.protocol === 'https:') e.setAttribute(name, url.href);
        else e.removeAttribute(name);
      }
    }
  });
  return JSON.stringify({locked, body: root.innerHTML});
})()
