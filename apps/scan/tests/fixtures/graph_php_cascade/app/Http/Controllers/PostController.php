<?php

namespace App\Http\Controllers;

use App\Models\Post;
use App\Services\PostService;

class PostController
{
    public function show(PostService $service): Post
    {
        return $service->load();
    }
}
