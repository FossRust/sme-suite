docker run -d \
  --name sme-suite \
  -e POSTGRES_DB=sme_suite \
  -e POSTGRES_USER=sme_suite \
  -e POSTGRES_PASSWORD=sme_suite \
  -p 5432:5432 \
  postgres:17
