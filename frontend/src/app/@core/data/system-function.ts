export interface SystemFunction {
  id: number;
  clusterId: number;
  categoryName: string;
  functionName: string;
  description: string;
  sqlQuery: string;
  displayOrder: number;
  categoryOrder: number;
  isFavorited: boolean;
  isSystem: boolean; // 数据库字段，标识是否为系统默认功能
  createdBy: number;
  createdAt: string;
  updatedAt: string;
}

export interface FunctionCategory {
  name: string;
  functions: SystemFunction[];
  order: number;
}

export interface CreateFunctionRequest {
  category_name: string;
  function_name: string;
  description: string;
  sql_query: string;
}

export interface UpdateOrderRequest {
  functions: FunctionOrder[];
}

export interface FunctionOrder {
  id: number;
  displayOrder: number;
  categoryOrder: number;
}
