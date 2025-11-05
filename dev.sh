#!/bin/bash

cargo build --features frontend
docker compose -f docker-compose.yml -f docker-compose.dev.yml up -d --force-recreate nitro_repo
