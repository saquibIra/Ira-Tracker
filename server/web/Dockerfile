# Time Pilot web app: solidtime (AGPL-3.0, https://github.com/solidtime-io/solidtime) with
# the Time Pilot changes in this folder applied on top of the official image.
#
#   overlay/      files added to, or replacing files in, the solidtime source tree
#   patches.mjs   small text edits; the build fails if an upstream text has changed
#
# Updating solidtime: change SOLIDTIME_VERSION, build, and fix any patch the build reports.
ARG SOLIDTIME_VERSION=0.23.0

FROM solidtime/solidtime:${SOLIDTIME_VERSION} AS upstream

# ---- 1. Patched source and frontend build --------------------------------------------
FROM node:22-bookworm-slim AS assets
WORKDIR /src
COPY --from=upstream /var/www/html/package.json /var/www/html/package-lock.json /var/www/html/.npmrc ./
COPY --from=upstream /var/www/html/resources/js/packages/ui/package.json resources/js/packages/ui/package.json
COPY --from=upstream /var/www/html/resources/js/packages/api/package.json resources/js/packages/api/package.json
RUN npm ci --no-audit --no-fund

COPY --from=upstream /var/www/html/app ./app
COPY --from=upstream /var/www/html/config ./config
COPY --from=upstream /var/www/html/routes ./routes
COPY --from=upstream /var/www/html/resources ./resources
COPY --from=upstream /var/www/html/public ./public
COPY --from=upstream /var/www/html/extensions ./extensions
COPY --from=upstream /var/www/html/lang ./lang
COPY --from=upstream /var/www/html/vite.config.js /var/www/html/vite-module-loader.js /var/www/html/components.json ./
# Build configs the published image leaves out, copied from the same solidtime tag.
COPY upstream/ ./
# The frontend imports Ziggy (route names) from the PHP vendor folder.
COPY --from=upstream /var/www/html/vendor/tightenco/ziggy ./vendor/tightenco/ziggy

COPY overlay/ ./
COPY patches.mjs /tmp/patches.mjs
RUN node /tmp/patches.mjs /src && rm -rf public/build \
    && VITE_APP_NAME="Time Pilot" npm run build

# ---- 2. The image: upstream plus the patched PHP, views and built assets --------------
FROM upstream
COPY --from=assets --chown=laravel:laravel /src/app /var/www/html/app
COPY --from=assets --chown=laravel:laravel /src/config /var/www/html/config
COPY --from=assets --chown=laravel:laravel /src/routes /var/www/html/routes
COPY --from=assets --chown=laravel:laravel /src/resources/views /var/www/html/resources/views
COPY --from=assets --chown=laravel:laravel /src/public /var/www/html/public
LABEL org.opencontainers.image.title="Time Pilot web" \
      org.opencontainers.image.licenses="AGPL-3.0-or-later" \
      org.opencontainers.image.description="solidtime with Time Pilot branding and onboarding"
