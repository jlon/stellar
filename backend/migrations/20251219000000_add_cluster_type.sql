-- ===========================================
-- Migration: Add cluster_type field to clusters table
-- ===========================================
-- Purpose: Support multiple OLAP engines (StarRocks, Doris)
-- Created: 2025-12-19

-- Add cluster_type column with default value 'starrocks'
-- Possible values: 'starrocks' | 'doris'
ALTER TABLE clusters ADD COLUMN cluster_type VARCHAR(20) DEFAULT 'starrocks' NOT NULL;

-- Create index for cluster_type filtering
CREATE INDEX IF NOT EXISTS idx_clusters_cluster_type ON clusters(cluster_type);

