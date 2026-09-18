-- Store initial-user passwords outside request_details and remove unsafe historical previews.
ALTER TABLE permission_requests ADD COLUMN new_user_password_encrypted TEXT NULL;

UPDATE permission_requests
SET executed_sql = NULL
WHERE executed_sql LIKE '%IDENTIFIED BY%';
