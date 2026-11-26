FROM node:24-bookworm-slim@sha256:dc26ae92c9fb15cf6b1d63e20f0944b2efe0f115b20e00be48fb23fceb061c54 AS build
WORKDIR /app
# MIT-licensed OpenDashboards fork, pinned by commit and archive checksum.
# The fork adds operator-authorized private data sources; production adapters stay here.
ADD --checksum=sha256:225cac929b2c77cf5c048e5d08924966a7db92dcce9448a8ccadee89e6156ac3 https://codeload.github.com/junr03/opendashboards/tar.gz/7aa12c4d47a7816f99f15b2ddc8b0a9ca481fe2c /tmp/upstream.tar.gz
RUN tar -xzf /tmp/upstream.tar.gz --strip-components=1 && rm /tmp/upstream.tar.gz
RUN corepack enable && corepack prepare pnpm@9.15.4 --activate
RUN pnpm install --frozen-lockfile
COPY overlay /overlay
COPY self-hosted ./self-hosted
COPY apply-overlay.mjs /tmp/apply-overlay.mjs
RUN node /tmp/apply-overlay.mjs && pnpm exec vitest run --config self-hosted/vitest.config.mts
ENV NEXT_TELEMETRY_DISABLED=1
RUN pnpm build

FROM build AS production-dependencies
RUN pnpm prune --prod

FROM node:24-bookworm-slim@sha256:dc26ae92c9fb15cf6b1d63e20f0944b2efe0f115b20e00be48fb23fceb061c54
WORKDIR /app
ENV NODE_ENV=production NEXT_TELEMETRY_DISABLED=1 PORT=3000
COPY --from=production-dependencies --chown=1000:100 /app /app
USER 1000:100
EXPOSE 3000
ENTRYPOINT ["node", "self-hosted/entrypoint.mjs"]
