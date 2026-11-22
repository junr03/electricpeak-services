FROM node:24-bookworm-slim@sha256:dc26ae92c9fb15cf6b1d63e20f0944b2efe0f115b20e00be48fb23fceb061c54 AS build
WORKDIR /app
# MIT-licensed upstream, pinned by commit and archive checksum. Upstream does
# not publish a production image or a standalone authentication gateway.
ADD --checksum=sha256:900a14d69b9c3f22e9ea3813dffa8e2e4bbd61f92e4544f20d45d3fddf72ded5 https://codeload.github.com/thesysdev/opendashboards/tar.gz/4c49e8d207fbb7407e0a0d1897dc101f09add3f0 /tmp/upstream.tar.gz
RUN tar -xzf /tmp/upstream.tar.gz --strip-components=1 && rm /tmp/upstream.tar.gz
RUN corepack enable && corepack prepare pnpm@9.15.4 --activate
RUN pnpm install --frozen-lockfile
COPY overlay /overlay
COPY self-hosted ./self-hosted
COPY apply-overlay.mjs /tmp/apply-overlay.mjs
RUN node /tmp/apply-overlay.mjs && pnpm exec vitest run --config self-hosted/vitest.config.mts
ENV NEXT_TELEMETRY_DISABLED=1
RUN pnpm build && pnpm prune --prod

FROM node:24-bookworm-slim@sha256:dc26ae92c9fb15cf6b1d63e20f0944b2efe0f115b20e00be48fb23fceb061c54
WORKDIR /app
ENV NODE_ENV=production NEXT_TELEMETRY_DISABLED=1 PORT=3000
COPY --from=build --chown=1000:100 /app /app
USER 1000:100
EXPOSE 3000
ENTRYPOINT ["node", "self-hosted/entrypoint.mjs"]
