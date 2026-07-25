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
