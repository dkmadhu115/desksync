You are a Principal Software Engineer, Staff Architect, DevOps Engineer, Security Engineer, Mobile Engineer, Desktop Engineer, and UI/UX Expert.

Your task is to build a production-grade application from scratch.

Do NOT generate simplified examples.

Do NOT skip implementation details.

Always generate enterprise-quality code.

Follow Clean Architecture, SOLID Principles, DDD where applicable, Repository Pattern, Dependency Injection, Interface Segregation, and production coding standards.

====================================================
PROJECT NAME
====================================================

Developer Remote Workstation

====================================================
PROJECT GOAL
====================================================

Build a secure remote desktop application that allows developers to completely control their own laptop from a mobile phone.

The application is NOT AI-based.

It simply provides a secure encrypted remote desktop.

The laptop becomes accessible from anywhere in the world whenever

• Laptop is powered ON
• Laptop has internet
• Mobile has internet
• Devices are already paired

If any device loses internet, no actions should execute.

When internet comes back, reconnect automatically.

====================================================
SUPPORTED PLATFORMS
====================================================

Desktop Agent

• Windows
• macOS
• Linux

Mobile

• Android
• iOS

Backend

Cloud Deployable

====================================================
TECH STACK
====================================================

Desktop Agent
-------------
Rust

Tokio

Tauri only for configuration UI

Windows API

ScreenCaptureKit

PipeWire

WebRTC

Tokio Tungstenite

Serde

Reqwest

SQLite

Keyring

Rustls

AES-GCM

X25519

Flutter Desktop only if absolutely necessary.

====================================================

Mobile

Flutter

Riverpod

Go Router

Dio

WebRTC

Firebase Messaging

Flutter Secure Storage

Hive

====================================================

Backend

Go

Fiber Framework

JWT

PostgreSQL

Redis

WebSocket

WebRTC Signaling

Coturn

Docker

Kubernetes

Helm

Prometheus

Grafana

Loki

OpenTelemetry

====================================================

Authentication

Google OAuth

GitHub OAuth

Email Login

JWT

Refresh Tokens

Device Certificates

====================================================

Streaming

WebRTC

VP9

H264

H265

Adaptive Bitrate

Hardware Encoding

====================================================

Database

PostgreSQL

Redis

SQLite (Desktop Cache)

====================================================
SYSTEM ARCHITECTURE
====================================================

Design the application using microservices.

Services

API Gateway

Authentication Service

Device Service

Session Service

Notification Service

Pairing Service

Signaling Service

Relay Service

Monitoring Service

====================================================
FEATURES
====================================================

Authentication

Device Registration

QR Pairing

Manual Pairing Code

Trusted Devices

Persistent Pairing

Automatic Reconnect

Online Status

Offline Status

Heartbeat

Desktop Streaming

Audio Streaming

Clipboard Sync

Keyboard Input

Mouse Input

Touch Gestures

File Transfer

Downloads

Uploads

Multiple Monitor Support

Session Logs

Device Management

Notifications

Background Running

Automatic Startup

Remote Lock

Disconnect

Remove Device

Session Timeout

====================================================
DEVELOPER FEATURES
====================================================

Quick Launch

VS Code

Cursor

Claude Desktop

Warp

Terminal

PowerShell

iTerm

Windows Terminal

Open specific project folders

Save favorite workspaces

Docker shortcuts

Git shortcuts

Kubectl shortcuts

Helm shortcuts

SSH shortcuts

====================================================
NETWORK
====================================================

Use WebRTC.

If direct connection fails

Automatically use TURN relay.

Use STUN.

Implement ICE Candidate Exchange.

Use secure WebSocket Signaling.

====================================================
SECURITY
====================================================

Everything must be encrypted.

AES-256 GCM

ECDH

Curve25519

Private keys never leave devices.

Server stores only public keys.

Implement

Certificate Pinning

Replay Protection

Nonce Validation

Session Expiration

Biometric Unlock on Mobile

PIN Protection

Device Revocation

Rate Limiting

Brute Force Protection

====================================================
SCREEN STREAMING
====================================================

Capture desktop

Compress

Encode

Send

Decode

Render

Support

720p

1080p

2K

4K

30 FPS

60 FPS

Adaptive FPS

Bandwidth adaptation

Hardware encoding

====================================================
INPUT
====================================================

Touch

Mouse

Trackpad

Keyboard

Special Keys

Ctrl

Alt

Shift

Cmd

Clipboard

Drag

Drop

Pinch Zoom

Right Click

Scroll

====================================================
FILE TRANSFER
====================================================

Drag files

Upload

Download

Resume

Progress

Pause

Cancel

Integrity Verification

====================================================
MONITORING
====================================================

Prometheus

Grafana

Loki

Distributed Tracing

OpenTelemetry

Health Endpoints

Metrics

====================================================
LOGGING
====================================================

Structured JSON Logs

Request IDs

Correlation IDs

Audit Logs

====================================================
DEPLOYMENT
====================================================

Everything must run using

Docker

Docker Compose

Kubernetes

Helm Charts

GitHub Actions

====================================================
TESTING
====================================================

Unit Tests

Integration Tests

Load Tests

End-to-End Tests

Security Tests

====================================================
CI/CD
====================================================

GitHub Actions

Lint

Tests

Coverage

Docker Build

Push Images

Deploy to Kubernetes

Rollback

====================================================
PROJECT STRUCTURE
====================================================

Design an enterprise repository.

Example

root/

mobile/

desktop-agent/

backend/

gateway/

auth/

device/

session/

pairing/

signaling/

relay/

notification/

helm/

docker/

docs/

terraform/

monitoring/

scripts/

====================================================
DOCUMENTATION
====================================================

Generate

Architecture diagrams

Sequence diagrams

API documentation

ER diagrams

Deployment diagrams

Developer Guide

Admin Guide

====================================================
IMPLEMENTATION STRATEGY
====================================================

Do NOT generate the entire project in one response.

Instead implement the application phase-by-phase.

Each phase must be production ready.

At the end of every phase

Wait for my approval.

====================================================
PHASES
====================================================

Phase 1

Project Planning

Architecture

Folder Structure

Database Design

API Design

Security Design

ER Diagram

Sequence Diagram

Component Diagram

Deployment Diagram

Threat Model

====================================================

Phase 2

Backend Foundation

Authentication

JWT

Database

Redis

Logging

Configuration

====================================================

Phase 3

Desktop Agent

Rust

System Services

Screen Capture

Keyboard Injection

Mouse Injection

Clipboard

====================================================

Phase 4

Flutter Mobile

Authentication

Pairing

Device List

Desktop Viewer

Touch Controls

====================================================

Phase 5

WebRTC

Signaling

Peer Connection

Streaming

Adaptive Bitrate

====================================================

Phase 6

Device Pairing

QR Code

Device Trust

Persistent Pairing

====================================================

Phase 7

Remote Desktop

Desktop Rendering

Keyboard

Mouse

Clipboard

====================================================

Phase 8

Developer Features

VS Code

Cursor

Claude

Git

Docker

Kubectl

SSH

====================================================

Phase 9

Security Hardening

Certificates

Replay Protection

Encryption

====================================================

Phase 10

Production Deployment

Docker

Kubernetes

Helm

CI/CD

Monitoring

====================================================
CODING RULES
====================================================

Never write pseudo code.

Always generate complete files.

Always explain why files exist.

Always use production folder structures.

Always include error handling.

Always include retries.

Always include logging.

Always include metrics.

Always include tests.

Always include documentation.

Always explain architectural decisions.

====================================================
IMPORTANT
====================================================

Act like a senior engineering team.

Challenge poor architectural decisions.

Recommend better alternatives when appropriate.

Optimize for maintainability, scalability, and security.

Never sacrifice security for simplicity.

Begin with Phase 1 only.

Wait for my approval before continuing to the next phase.