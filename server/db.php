<?php

require_once __DIR__ . '/config.php';

// Never leak stack traces / filesystem paths to clients.
ini_set('display_errors', '0');
set_exception_handler(function (Throwable $e): void {
    error_log('[gootif] ' . $e::class . ': ' . $e->getMessage());
    json_response(['error' => 'Internal server error'], 500);
});

function get_db(): PDO {
    static $pdo = null;
    if ($pdo === null) {
        $dir = dirname(DB_PATH);
        if (!is_dir($dir)) {
            mkdir($dir, 0700, true);
        }
        $pdo = new PDO('sqlite:' . DB_PATH);
        $pdo->setAttribute(PDO::ATTR_ERRMODE, PDO::ERRMODE_EXCEPTION);
        $pdo->setAttribute(PDO::ATTR_DEFAULT_FETCH_MODE, PDO::FETCH_ASSOC);
        $pdo->exec('PRAGMA journal_mode=WAL');
        init_db($pdo);
    }
    return $pdo;
}

function init_db(PDO $pdo): void {
    $pdo->exec("
        CREATE TABLE IF NOT EXISTS notifications (
            id TEXT PRIMARY KEY,
            service TEXT NOT NULL,
            title TEXT NOT NULL,
            body TEXT DEFAULT '',
            priority TEXT DEFAULT 'medium' CHECK(priority IN ('low','medium','high')),
            metadata TEXT DEFAULT '{}',
            created_at TEXT NOT NULL
        )
    ");
    $pdo->exec("CREATE INDEX IF NOT EXISTS idx_service ON notifications(service)");
    $pdo->exec("CREATE INDEX IF NOT EXISTS idx_created_at ON notifications(created_at DESC)");
}
