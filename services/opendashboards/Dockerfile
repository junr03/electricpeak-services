FROM node:24-bookworm-slim@sha256:dc26ae92c9fb15cf6b1d63e20f0944b2efe0f115b20e00be48fb23fceb061c54 AS build
WORKDIR /app
# MIT-licensed OpenDashboards fork, pinned by commit and archive checksum.
# The fork owns inference, generation, script execution, history and authentication.
ADD --checksum=sha256:f06b4756a81833e49a7d68017d41dd1ee3649f41ea49e446ebb3c092700e36ee https://codeload.github.com/junr03/opendashboards/tar.gz/9c9279cdbe4e1da9138632571919b4a9bdf586b1 /tmp/upstream.tar.gz
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
