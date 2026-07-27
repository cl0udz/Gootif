<?php

require_once __DIR__ . '/../db.php';

if ($_SERVER['REQUEST_METHOD'] !== 'GET') {
    json_response(['error' => 'Method not allowed'], 405);
}

verify_app_key();

$pdo = get_db();
$stmt = $pdo->query("
    SELECT
        service AS name,
        COUNT(*) AS notification_count,
        MAX(created_at) AS last_notification_at
    FROM notifications
    GROUP BY service
    ORDER BY last_notification_at DESC
");

json_response($stmt->fetchAll());
