<?php

// Copy to config.php and fill in your own random keys, e.g.:
//   php -r "echo bin2hex(random_bytes(24)), PHP_EOL;"
define('SERVICE_KEY', 'REPLACE_WITH_YOUR_SERVICE_KEY');
define('APP_KEY', 'REPLACE_WITH_YOUR_APP_KEY');
// Best: point this OUTSIDE the webroot (e.g. dirname(__DIR__) . '/gootif-data/gootif.db')
// so the DB can never be served over HTTP. If it stays inside, /data/ is blocked
// by .htaccess rules — see DEPLOYMENT.md.
define('DB_PATH', __DIR__ . '/data/gootif.db');

function json_response($data, int $status = 200): void {
    http_response_code($status);
    header('Content-Type: application/json');
    echo json_encode($data);
    exit;
}

function verify_service_key(): void {
    $key = $_SERVER['HTTP_X_SERVICE_KEY'] ?? '';
    if (!hash_equals(SERVICE_KEY, $key)) {
        json_response(['error' => 'Unauthorized'], 401);
    }
}

function verify_app_key(): void {
    $key = $_SERVER['HTTP_X_API_KEY'] ?? '';
    if (!hash_equals(APP_KEY, $key)) {
        json_response(['error' => 'Unauthorized'], 401);
    }
}

function decode_metadata(string $json): object|array {
    $decoded = json_decode($json);
    return $decoded ?? new \stdClass();
}

function get_request_body(): array {
    $body = file_get_contents('php://input');
    $data = json_decode($body, true);
    if (!is_array($data)) {
        json_response(['error' => 'Invalid JSON body'], 400);
    }
    return $data;
}
