<?php

require_once __DIR__ . '/../db.php';

$method = $_SERVER['REQUEST_METHOD'];
$id = $_GET['id'] ?? null;

if ($method === 'POST' && !$id) {
    verify_service_key();

    $data = get_request_body();

    if (empty($data['service']) || empty($data['title'])) {
        json_response(['error' => '"service" and "title" are required'], 400);
    }

    $id = bin2hex(random_bytes(16));
    $now = gmdate('Y-m-d\TH:i:s\Z');
    $priority = $data['priority'] ?? 'medium';
    if (!in_array($priority, ['low', 'medium', 'high'])) {
        $priority = 'medium';
    }
    $metadata = isset($data['metadata']) ? json_encode($data['metadata']) : '{}';

    $pdo = get_db();
    $stmt = $pdo->prepare("
        INSERT INTO notifications (id, service, title, body, priority, metadata, created_at)
        VALUES (:id, :service, :title, :body, :priority, :metadata, :created_at)
    ");
    $stmt->execute([
        ':id' => $id,
        ':service' => $data['service'],
        ':title' => $data['title'],
        ':body' => $data['body'] ?? '',
        ':priority' => $priority,
        ':metadata' => $metadata,
        ':created_at' => $now,
    ]);

    json_response([
        'id' => $id,
        'service' => $data['service'],
        'title' => $data['title'],
        'body' => $data['body'] ?? '',
        'priority' => $priority,
        'metadata' => decode_metadata($metadata),
        'created_at' => $now,
    ], 201);

} elseif ($method === 'GET' && $id) {
    verify_app_key();

    $pdo = get_db();
    $stmt = $pdo->prepare("SELECT * FROM notifications WHERE id = :id");
    $stmt->execute([':id' => $id]);
    $row = $stmt->fetch();

    if (!$row) {
        json_response(['error' => 'Not found'], 404);
    }

    $row['metadata'] = decode_metadata($row['metadata']);
    json_response($row);

} elseif ($method === 'GET' && !$id) {
    verify_app_key();

    $service = $_GET['service'] ?? null;
    $limit = min((int)($_GET['limit'] ?? 50), 200);
    $offset = max((int)($_GET['offset'] ?? 0), 0);
    $since = $_GET['since'] ?? null;

    $pdo = get_db();
    $where = [];
    $params = [];

    if ($service) {
        $where[] = 'service = :service';
        $params[':service'] = $service;
    }
    if ($since) {
        $where[] = 'created_at > :since';
        $params[':since'] = $since;
    }

    $whereClause = $where ? 'WHERE ' . implode(' AND ', $where) : '';
    $sql = "SELECT * FROM notifications $whereClause ORDER BY created_at DESC LIMIT :limit OFFSET :offset";

    $stmt = $pdo->prepare($sql);
    foreach ($params as $k => $v) {
        $stmt->bindValue($k, $v);
    }
    $stmt->bindValue(':limit', $limit, PDO::PARAM_INT);
    $stmt->bindValue(':offset', $offset, PDO::PARAM_INT);
    $stmt->execute();

    $rows = $stmt->fetchAll();
    foreach ($rows as &$row) {
        $row['metadata'] = decode_metadata($row['metadata']);
    }

    json_response($rows);

} elseif ($method === 'DELETE' && $id) {
    verify_app_key();

    $pdo = get_db();
    $stmt = $pdo->prepare("DELETE FROM notifications WHERE id = :id");
    $stmt->execute([':id' => $id]);

    if ($stmt->rowCount() === 0) {
        json_response(['error' => 'Not found'], 404);
    }

    http_response_code(204);
    exit;

} else {
    json_response(['error' => 'Method not allowed'], 405);
}
