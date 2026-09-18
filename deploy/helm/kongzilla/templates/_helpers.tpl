{{- define "kongzilla.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" }}
{{- end }}

{{- define "kongzilla.fullname" -}}
{{- if .Values.fullnameOverride }}
{{- .Values.fullnameOverride | trunc 63 | trimSuffix "-" }}
{{- else }}
{{- $name := default .Chart.Name .Values.nameOverride }}
{{- if contains $name .Release.Name }}
{{- .Release.Name | trunc 63 | trimSuffix "-" }}
{{- else }}
{{- printf "%s-%s" .Release.Name $name | trunc 63 | trimSuffix "-" }}
{{- end }}
{{- end }}
{{- end }}

{{- define "kongzilla.chart" -}}
{{- printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" | trunc 63 | trimSuffix "-" }}
{{- end }}

{{- define "kongzilla.selectorLabels" -}}
app.kubernetes.io/name: {{ include "kongzilla.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end }}

{{/*
The site's own pods, and nothing else.

A selector matches every pod carrying at least these labels, so selecting on the
release alone reaches the beacon too - it is part of the same release and wears
the same two labels plus one of its own. The site's service would then send one
request in three to a process that only knows `/api/event`, and answers 404 to
everything else.
*/}}
{{- define "kongzilla.siteSelectorLabels" -}}
{{ include "kongzilla.selectorLabels" . }}
app.kubernetes.io/component: web
{{- end }}

{{- define "kongzilla.labels" -}}
helm.sh/chart: {{ include "kongzilla.chart" . }}
{{ include "kongzilla.selectorLabels" . }}
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
{{- end }}

{{- define "kongzilla.image" -}}
{{- if .Values.image.digest -}}
{{ printf "%s@%s" .Values.image.repository .Values.image.digest }}
{{- else -}}
{{ printf "%s:%s" .Values.image.repository .Values.image.tag }}
{{- end -}}
{{- end }}
