# Stellar Helm Chart

This Helm chart deploys Stellar on Kubernetes. It provides a scalable and configurable way to run Stellar in a Kubernetes cluster.

## Prerequisites

- Kubernetes 1.16+
- Helm 3.0+
- Ingress controller (if using ingress)

## Installing the Chart

To install the chart with the release name `stellar`:

```bash
helm install stellar .
```

This command deploys Stellar on the Kubernetes cluster with the default configuration.

## Uninstalling the Chart

To uninstall/delete the `stellar` deployment:

```bash
helm delete stellar
```

## Configuration

The following table lists the configurable parameters of the Stellar chart and their default values.

| Parameter | Description | Default |
| --------- | ----------- | ------- |
| `replicaCount` | Number of replicas | `1` |
| `image.repository` | Image repository | `ghcr.io/jlon/stellar` |
| `image.pullPolicy` | Image pull policy | `IfNotPresent` |
| `image.tag` | Image tag (defaults to chart appVersion) | `"latest"` |
| `service.type` | Kubernetes service type | `ClusterIP` |
| `service.port` | Service port | `8080` |
| `ingress.enabled` | Enable ingress | `true` |
| `ingress.hosts` | Ingress hosts configuration | `[host: stellar-example.local, paths: [{path: /stellar(/|$)(.*), pathType: Prefix}]` |
| `persistence.enabled` | Enable persistence | `true` |
| `persistence.size` | Persistence volume size | `10Gi` |
| `resources` | CPU/Memory resource requests/limits | `{}` |
| `nodeSelector` | Node labels for pod assignment | `{}` |
| `tolerations` | Tolerations for pod assignment | `[]` |
| `affinity` | Affinity for pod assignment | `{}` |

### JWT Secret

By default, the chart will generate a random JWT secret for authentication. You can specify a custom JWT secret by setting the `jwtSecret` value:

```bash
helm install my-release . --set jwtSecret="your-custom-secret"
```

Or in your `values.yaml`:

```yaml
jwtSecret: "your-custom-secret"
```

For production, it's recommended to generate a strong secret:

```bash
openssl rand -base64 32
```

### Ingress Configuration

To configure ingress, you can modify the values in `values.yaml`:

```yaml
ingress:
  enabled: true
  className: ""
  annotations:
    nginx.ingress.kubernetes.io/rewrite-target: /$2
  hosts:
    - host: starrocks.yourdomain.com
      paths:
        - path: /stellar(/|$)(.*)
          pathType: Prefix
  tls: []
```

### Resource Configuration

You can specify resource requests and limits for the deployment:

```yaml
resources:
  limits:
    cpu: 1000m
    memory: 1Gi
  requests:
    cpu: 250m
    memory: 256Mi
```

## Persistence

The chart uses a PersistentVolumeClaim for storing data and logs. By default, it uses the default storage class with 10Gi storage.

The following directories are persisted:
- `/app/data` - Database files
- `/app/logs` - Log files
- `/app/conf` - Configuration files (read-only from ConfigMap)

You can customize persistence settings in `values.yaml`:

```yaml
persistence:
  enabled: true
  storageClassName: ""
  existingClaim: ""
  accessModes:
    - ReadWriteOnce
  size: 10Gi
  annotations: {}
```

## Accessing the Application

After deploying the chart, you can access the application in several ways depending on your configuration:

1. **Using Port Forwarding**:
   ```bash
   kubectl port-forward svc/<release-name>-stellar 8080:8080
   ```
   Then open http://localhost:8080

2. **Using Ingress** (if configured):
   If you have configured ingress, you can access the application using the configured hostname and path.

3. **Using LoadBalancer Service Type**:
   If you set `service.type=LoadBalancer`, you can access the application using the external IP assigned to the service.

## Upgrading the Chart

To upgrade the chart with new values:

```bash
helm upgrade stellar . -f values.yaml
```

## Values File

See [values.yaml](./values.yaml) for the complete list of configurable values.