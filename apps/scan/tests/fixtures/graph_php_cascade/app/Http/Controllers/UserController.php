<?php

namespace App\Http\Controllers;

use App\Models\User;
use App\Services\UserService;

class UserController
{
    public function show(UserService $service): User
    {
        return $service->load();
    }
}
