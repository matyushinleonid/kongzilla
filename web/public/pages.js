/*
 * The written pages report their own visit.
 *
 * These are the pages search engines land on, so they are where the referrer is
 * worth knowing - and they carry no application code, so they cannot borrow the
 * app's beacon. Fifteen lines, same origin, fire and forget.
 *
 * Not part of the app bundle and not part of the file people download: this
 * lives in public/, which the standalone build does not copy.
 */
(function () {
  if (location.protocol !== "https:" && location.hostname !== "localhost") return;
  var body = JSON.stringify({
    name: "pageview",
    path: location.pathname,
    referrer: document.referrer,
  });
  try {
    if (navigator.sendBeacon) {
      navigator.sendBeacon("/api/event", new Blob([body], { type: "application/json" }));
      return;
    }
    fetch("/api/event", {
      method: "POST",
      body: body,
      headers: { "Content-Type": "application/json" },
      keepalive: true,
    }).catch(function () {});
  } catch (error) {
    /* Statistics are never worth an error in front of the reader. */
  }
})();
