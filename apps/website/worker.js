// One canonical address: https://turtle-flow.com. Anything on www or plain http gets a single
// permanent redirect to it (path and query kept); everything else is served from the static
// assets.
const CANONICAL_HOST = 'turtle-flow.com';

export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    if (url.hostname === `www.${CANONICAL_HOST}` || url.protocol === 'http:') {
      url.hostname = CANONICAL_HOST;
      url.protocol = 'https:';
      return Response.redirect(url.toString(), 301);
    }
    return env.ASSETS.fetch(request);
  },
};
