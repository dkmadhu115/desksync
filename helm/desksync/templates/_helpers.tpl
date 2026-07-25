{{/*
Chart name (overridable).
*/}}
{{- define "desksync.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{/*
Fully-qualified release name (overridable).
*/}}
{{- define "desksync.fullname" -}}
{{- if .Values.fullnameOverride -}}
{{- .Values.fullnameOverride | trunc 63 | trimSuffix "-" -}}
{{- else -}}
{{- $name := default .Chart.Name .Values.nameOverride -}}
{{- if contains $name .Release.Name -}}
{{- .Release.Name | trunc 63 | trimSuffix "-" -}}
{{- else -}}
{{- printf "%s-%s" .Release.Name $name | trunc 63 | trimSuffix "-" -}}
{{- end -}}
{{- end -}}
{{- end -}}

{{/*
Chart label value.
*/}}
{{- define "desksync.chart" -}}
{{- printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{/*
Common labels.
*/}}
{{- define "desksync.labels" -}}
helm.sh/chart: {{ include "desksync.chart" . }}
{{ include "desksync.selectorLabels" . }}
{{- if .Chart.AppVersion }}
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
{{- end }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
app.kubernetes.io/part-of: desksync
{{- end -}}

{{/*
Selector labels (stable across upgrades).
*/}}
{{- define "desksync.selectorLabels" -}}
app.kubernetes.io/name: {{ include "desksync.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end -}}

{{/*
ServiceAccount name.
*/}}
{{- define "desksync.serviceAccountName" -}}
{{- if .Values.serviceAccount.create -}}
{{- default (include "desksync.fullname" .) .Values.serviceAccount.name -}}
{{- else -}}
{{- default "default" .Values.serviceAccount.name -}}
{{- end -}}
{{- end -}}

{{/*
Name of the Secret holding shared secrets.
*/}}
{{- define "desksync.secretName" -}}
{{- if .Values.secrets.existingSecret -}}
{{- .Values.secrets.existingSecret -}}
{{- else -}}
{{- printf "%s-secrets" (include "desksync.fullname" .) -}}
{{- end -}}
{{- end -}}

{{/*
Effective image tag (defaults to the chart appVersion).
*/}}
{{- define "desksync.imageTag" -}}
{{- .Values.global.image.tag | default .Chart.AppVersion -}}
{{- end -}}

{{/*
Build an image reference from (registry, repository, name, tag), omitting any
empty registry/repository segments. This lets the chart reference public
registry images (ghcr.io/owner/repo/name:tag) as well as images imported
directly into the node's container runtime (e.g. desksync/name:tag or
name:tag) with no registry prefix.
Usage: {{ include "desksync.imageRef" (list $registry $repository $name $tag) }}
*/}}
{{- define "desksync.imageRef" -}}
{{- $registry := index . 0 -}}
{{- $repository := index . 1 -}}
{{- $name := index . 2 -}}
{{- $tag := index . 3 -}}
{{- $ref := printf "%s:%s" $name $tag -}}
{{- if $repository -}}{{- $ref = printf "%s/%s" $repository $ref -}}{{- end -}}
{{- if $registry -}}{{- $ref = printf "%s/%s" $registry $ref -}}{{- end -}}
{{- $ref -}}
{{- end -}}

{{/*
Effective DATABASE_URL. When the in-cluster Postgres is enabled it is derived
from postgres.auth so the app, migrations, and the database always agree;
otherwise the operator-supplied secrets.data.DATABASE_URL is used.
*/}}
{{- define "desksync.databaseUrl" -}}
{{- if .Values.postgres.enabled -}}
{{- $a := .Values.postgres.auth -}}
{{- printf "postgres://%s:%s@postgres:5432/%s?sslmode=disable" $a.username $a.password $a.database -}}
{{- else -}}
{{- .Values.secrets.data.DATABASE_URL -}}
{{- end -}}
{{- end -}}

{{/*
Effective REDIS_ADDR (in-cluster redis service or operator-supplied value).
*/}}
{{- define "desksync.redisAddr" -}}
{{- if .Values.redis.enabled -}}redis:6379{{- else -}}{{- .Values.secrets.data.REDIS_ADDR -}}{{- end -}}
{{- end -}}
