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

    // Generous caps — agent reports live in metadata and can be large, but
    // nothing legitimate in this ecosystem exceeds these.
    if (!is_string($data['service']) || strlen($data['service']) > 100) {
        json_response(['error' => '"service" must be a string of at most 100 bytes'], 400);
    }
    if (!is_string($data['title']) || strlen($data['title']) > 500) {
        json_response(['error' => '"title" must be a string of at most 500 bytes'], 400);
    }
    if (isset($data['body']) && (!is_string($data['body']) || strlen($data['body']) > 65536)) {
        json_response(['error' => '"body" must be a string of at most 64 KB'], 400);
    }
    if (isset($data['metadata']) && !is_array($data['metadata'])) {
        json_response(['error' => '"metadata" must be a JSON object'], 400);
    }

    $id = bin2hex(random_bytes(16));
    $now = gmdate('Y-m-d\TH:i:s\Z');
    $priority = $data['priority'] ?? 'medium';
    if (!in_array($priority, ['low', 'medium', 'high'])) {
        $priority = 'medium';
    }
    $metadata = isset($data['metadata']) ? json_encode($data['metadata']) : '{}';
    if ($metadata === false) {
        json_response(['error' => '"metadata" could not be encoded (too deeply nested?)'], 400);
    }
    if (strlen($metadata) > 1048576) {
        json_response(['error' => '"metadata" must encode to at most 1 MB'], 400);
    }

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
